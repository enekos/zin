const { createServer } = require("node:http");
const { once } = require("node:events");
const { Rejected } = require("./index.cjs");

/** FIFO by default. A predicate lets a later delivery overtake an earlier one. */
class Channel {
  constructor(name = "event") {
    this.name = name;
    this.held = [];
  }
  get pending() { return this.held.length; }
  hold(value) { this.held.push(value); }
  take(match) {
    const index = match ? this.held.findIndex(match) : 0;
    if (index < 0 || index >= this.pending) throw new Rejected(`no ${this.name} is waiting`);
    return this.held.splice(index, 1)[0];
  }
}

/** Hold incoming WebSocket frames. A reply function selects an in-process server. */
async function socketFrames(page, url, reply) {
  const channel = new Channel("WebSocket frame");
  await page.routeWebSocket(url, socket => {
    if (reply) {
      socket.onMessage(message => channel.hold({ socket, data: reply(message) }));
    } else {
      const server = socket.connectToServer();
      server.onMessage(data => channel.hold({ socket, data }));
    }
  });
  return {
    get pending() { return channel.pending; },
    release(match) {
      const frame = channel.take(match && (frame => match(frame.data)));
      frame.socket.send(frame.data);
    },
  };
}

/** Native EventSource endpoint for a test. Enqueue one event for its current subscribers. */
async function eventStream() {
  const clients = new Set();
  const channel = new Channel("SSE event");
  const server = createServer((request, stream) => {
    stream.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache",
      "Access-Control-Allow-Origin": "*" });
    stream.write(": connected\n\n");
    clients.add(stream);
    stream.on("close", () => clients.delete(stream));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    url: `http://127.0.0.1:${server.address().port}/events`,
    get pending() { return channel.pending; },
    hold(data, event = "message") {
      if (clients.size === 0) throw new Rejected("no EventSource is connected");
      if (/[\r\n]/.test(event)) throw new TypeError("SSE event names must occupy one line");
      channel.hold({ data, event, targets: [...clients] });
    },
    release(match) {
      const { data, event, targets } = channel.take(match && (frame => match(frame.data)));
      const lines = data.split(/\r\n|\r|\n/).map(line => `data: ${line}\n`).join("");
      for (const target of targets) if (clients.has(target)) target.write(`event: ${event}\n${lines}\n`);
    },
    async dispose() {
      for (const client of clients) client.end();
      const closed = once(server, "close");
      server.close();
      server.closeAllConnections();
      await closed;
    },
  };
}

async function clock(page, time = new Date("2020-01-01T00:00:00Z")) {
  // Start just before the requested instant so pauseAt never jumps backwards.
  await page.clock.install({ time: new Date(+time - 60000) });
  await page.clock.pauseAt(time);
  return { advance: milliseconds => page.clock.runFor(milliseconds) };
}

function navigation(page) {
  async function move(back) {
    const available = await page.evaluate(back => {
      if (!window.navigation) throw new Error("history actions require the Navigation API");
      return back ? navigation.canGoBack : navigation.canGoForward;
    }, back);
    if (!available) throw new Rejected("no history entry in that direction");
    if (back) await page.goBack();
    else await page.goForward();
  }
  return { back: () => move(true), forward: () => move(false), reload: () => page.reload() };
}

function connectivity(page) {
  async function change(online) {
    if (await page.evaluate(() => navigator.onLine) === online) throw new Rejected();
    await page.context().setOffline(!online);
  }
  return { offline: () => change(false), online: () => change(true) };
}

/** Give each finite actor a fresh browser context. Pass a context for same-session tabs. */
async function actors(target, values) {
  const pages = new Map();
  const owned = [];
  const dispose = async () => { await Promise.all(owned.splice(0).map(item => item.close())); };
  try {
    for (const actor of values) {
      if (pages.has(actor)) throw new Error(`duplicate actor ${actor}`);
      if (typeof target.newContext === "function") {
        const context = await target.newContext();
        owned.push(context);
        pages.set(actor, await context.newPage());
      } else {
        const page = await target.newPage();
        owned.push(page);
        pages.set(actor, page);
      }
    }
  } catch (error) {
    await dispose();
    throw error;
  }
  return { page(actor) {
    if (!pages.has(actor)) throw new Error(`unknown actor ${actor}`);
    return pages.get(actor);
  }, dispose };
}

module.exports = { Channel, socketFrames, eventStream, clock, navigation, connectivity, actors };
