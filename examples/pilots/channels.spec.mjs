import { expect, test } from "@playwright/test";
import { check, Divergence } from "zin-verify";
import { actors, clock, socketFrames } from "zin-verify/playwright";

const users = ["alice", "bob"];

async function debounce(browser, broken) {
  const page = await browser.newPage();
  const timer = await clock(page);
  await page.setContent('<button>Edit</button><output>0</output>');
  await page.evaluate(broken => {
    const button = document.querySelector("button");
    window.draft = 0;
    let timeout;
    button.onclick = () => {
      const value = ++window.draft;
      button.disabled = value === 2;
      if (!broken) clearTimeout(timeout);
      timeout = setTimeout(() => document.querySelector("output").value = value, 200);
    };
  }, broken);
  return {
    actions: { edit: () => page.locator("button").evaluate(b => b.disabled ? false : b.click()),
      tick: () => timer.advance(100) },
    observe: () => page.evaluate(() => ({ draft, saved: Number(document.querySelector("output").value) })),
    teardown: () => page.close(),
  };
}

async function tabs(browser, broken) {
  const context = await browser.newContext();
  await context.route("http://tabs.test/**", route => route.fulfill({ contentType: "text/html", body: "<title>Editor</title>" }));
  const group = await actors(context, users);
  for (const user of users) {
    await group.page(user).goto("http://tabs.test/");
    await group.page(user).evaluate(() => window.seen = 0);
  }
  return {
    actions: {
      refresh: ({ u }) => group.page(u).evaluate(() => window.seen = Number(localStorage.revision || 0)),
      save: ({ u }) => group.page(u).evaluate(broken => {
        const revision = Number(localStorage.revision || 0);
        if (revision === 2 || (!broken && window.seen !== revision)) return false;
        window.seen = revision + 1;
        localStorage.revision = window.seen;
      }, broken),
    },
    observe: () => group.page("alice").evaluate(() => ({ revision: Number(localStorage.revision || 0) })),
    teardown: () => context.close(),
  };
}

async function frames(browser, broken) {
  const page = await browser.newPage();
  const replies = await socketFrames(page, "**/ws", request => request);
  await page.goto("about:blank");
  await page.evaluate(() => new Promise(resolve => {
    const socket = new WebSocket("ws://frames.test/ws");
    const pending = new Set();
    window.received = "none";
    window.request = n => {
      if (pending.has(n)) return false;
      pending.add(n);
      socket.send(n);
    };
    socket.onmessage = ({ data }) => { pending.delete(data); window.received = data; };
    socket.onopen = resolve;
  }));
  return {
    actions: {
      request: async ({ n }) => {
        const count = replies.pending;
        const result = await page.evaluate(n => window.request(n), n);
        if (result === false) return false;
        await expect.poll(() => replies.pending).toBe(count + 1);
      },
      deliver: ({ n }) => replies.release(broken ? undefined : data => data === n),
    },
    observe: () => page.evaluate(() => ({ received: window.received })),
    teardown: () => page.close(),
  };
}

for (const [name, factory] of Object.entries({ debounce, tabs, frames })) {
  test(`${name} accepts the correct browser and detects the faulty variant`, async ({ browser }) => {
    const spec = new URL(`./${name}.zin`, import.meta.url);
    const options = { cover: "scenarios", observeTimeout: 300 };
    await check(spec, () => factory(browser, false), options);
    await expect(check(spec, () => factory(browser, true), options)).rejects.toBeInstanceOf(Divergence);
  });
}
