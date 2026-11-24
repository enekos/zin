const { expect, test } = require("@playwright/test");
const { eventStream, navigation, connectivity, Rejected } = require("zin-verify/playwright");

test("native SSE events wait for release and keep their event name and lines", async ({ page }) => {
  const stream = await eventStream();
  try {
    const origin = new URL("/", stream.url).href;
    await page.route(origin, route => route.fulfill({ contentType: "text/html", body: "<title>SSE</title>" }));
    await page.goto(origin);
    await page.evaluate(url => new Promise(resolve => {
      const source = new EventSource(url);
      source.addEventListener("notice", event => window.notice = event.data);
      source.onopen = () => resolve();
    }), stream.url);
    stream.hold("first");
    stream.hold("two\nlines", "notice");
    stream.release(text => text.startsWith("two"));
    await expect.poll(() => page.evaluate(() => window.notice)).toBe("two\nlines");
    expect(stream.pending).toBe(1);
    expect(() => stream.release(() => false)).toThrow(Rejected);
  } finally { await stream.dispose(); }
});

test("history and connectivity actions refuse at the boundary", async ({ page }) => {
  await page.route("https://history.test/**", r => r.fulfill({ contentType: "text/html", body: "ok" }));
  await page.goto("https://history.test/");
  const nav = navigation(page);
  await page.evaluate(() => history.pushState({}, "", "/next"));
  await nav.back();
  expect(new URL(page.url()).pathname).toBe("/");
  await nav.forward();
  expect(new URL(page.url()).pathname).toBe("/next");
  await expect(nav.forward()).rejects.toThrow(Rejected);
  await nav.reload();
  const net = connectivity(page);
  await net.offline();
  await expect(net.offline()).rejects.toThrow(Rejected);
  await net.online();
});

test("message channels order native window and BroadcastChannel events", async ({ page }) => {
  const { messages } = require("zin-verify/playwright");
  const events = await messages(page, { broadcasts: ["changes"] });
  await page.route("https://messages.test/", r => r.fulfill({ contentType: "text/html", body: "<title>Messages</title>" }));
  await page.goto("https://messages.test/");
  await page.evaluate(() => {
    window.received = [];
    window.onmessage = e => received.push(e.data);
    window.receiver = new BroadcastChannel("changes");
    receiver.onmessage = e => received.push(e.data);
    window.postMessage("window", location.origin);
    new BroadcastChannel("changes").postMessage("broadcast");
  });
  await expect.poll(async () => (await events.pending()).length).toBe(2);
  const pending = await events.pending();
  await events.release(pending.indexOf("broadcast"));
  await events.release();
  expect(await page.evaluate(() => received)).toEqual(["broadcast", "window"]);
  await expect(events.release()).rejects.toThrow(Rejected);
});

test("dialog, visibility and permission changes are observable browser actions", async ({ page, context }) => {
  const { dialogs, permissions, visibility } = require("zin-verify/playwright");
  const origin = "https://events.test";
  await page.route(origin, r => r.fulfill({ contentType: "text/html", body: "<title>Events</title>" }));
  await page.goto(origin);
  const grants = permissions(context, origin);
  await grants.grant("geolocation");
  const permission = () => page.evaluate(async () => (await navigator.permissions.query({ name: "geolocation" })).state);
  expect(await permission()).toBe("granted");
  await grants.revoke("geolocation");
  expect(await permission()).not.toBe("granted");
  await grants.dispose();
  await visibility(page, true);
  expect(await page.evaluate(() => document.visibilityState)).toBe("hidden");
  await expect(visibility(page, true)).rejects.toThrow(Rejected);
  await visibility(page, false);

  const held = dialogs(page);
  const opened = page.waitForEvent("dialog");
  const answer = page.evaluate(() => prompt("name"));
  await opened;
  await held.accept("Alice");
  expect(await answer).toBe("Alice");
  expect(() => held.dismiss()).toThrow(Rejected);
  await held.dispose();
});
