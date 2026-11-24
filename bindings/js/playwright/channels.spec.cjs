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
