const { Channel } = require("./channels.cjs");
const { Rejected } = require("./index.cjs");

function dialogs(page) {
  const channel = new Channel("dialog");
  const hold = dialog => channel.hold(dialog);
  page.on("dialog", hold);
  return {
    get pending() { return channel.pending; },
    accept: value => channel.take().accept(value),
    dismiss: () => channel.take().dismiss(),
    async dispose() {
      page.off("dialog", hold);
      while (channel.pending) await channel.take().dismiss();
    },
  };
}

/** Own permission overrides for an isolated test context. */
function permissions(context, origin) {
  const granted = new Set();
  async function change(name, enable) {
    if (granted.has(name) === enable) throw new Rejected();
    const next = new Set(granted);
    if (enable) next.add(name);
    else next.delete(name);
    // Start from the browser's defaults. Context overrides accumulate grants.
    await context.clearPermissions();
    try {
      await context.grantPermissions([...next], { origin });
    } catch (error) {
      await context.grantPermissions([...granted], { origin });
      throw error;
    }
    if (enable) granted.add(name);
    else granted.delete(name);
  }
  return { grant: name => change(name, true), revoke: name => change(name, false),
    dispose: () => context.clearPermissions() };
}

/** Simulate the Page Visibility API in the current document. */
async function visibility(page, hidden) {
  const changed = await page.evaluate(hidden => {
    if (document.hidden === hidden) return false;
    Object.defineProperties(document, {
      hidden: { configurable: true, value: hidden },
      visibilityState: { configurable: true, value: hidden ? "hidden" : "visible" },
    });
    document.dispatchEvent(new Event("visibilitychange"));
    return true;
  }, hidden);
  if (!changed) throw new Rejected("document already has that visibility");
}

// Installed before application listeners. Retain the original receiver, source,
// origin and transferred ports when a held message is delivered.
function interceptMessages({ windowMessages, broadcasts, key }) {
  const queue = [];
  const released = new WeakSet();
  function hold(event) {
    if (released.has(event)) return;
    event.stopImmediatePropagation();
    queue.push([event.currentTarget, event]);
  }
  if (windowMessages) window.addEventListener("message", hold, true);
  if (broadcasts.length) {
    const Native = window.BroadcastChannel;
    window.BroadcastChannel = class extends Native {
      constructor(name) {
        super(name);
        if (broadcasts.includes(this.name)) this.addEventListener("message", hold, true);
      }
    };
  }
  window[key] = {
    inspect: () => queue.map(([, event]) => event.data),
    release(index) {
      if (index < 0 || index >= queue.length) return false;
      const [target, source] = queue.splice(index, 1)[0];
      const event = new MessageEvent("message", {
        data: source.data, origin: source.origin, source: source.source,
        ports: [...source.ports], lastEventId: source.lastEventId,
      });
      released.add(event);
      target.dispatchEvent(event);
      return true;
    },
  };
}

async function messages(page, { windowMessages = true, broadcasts = [] } = {}) {
  const key = "__zin_browser_messages__";
  await page.addInitScript(interceptMessages, { windowMessages, broadcasts, key });
  return {
    pending: () => page.evaluate(key => window[key].inspect(), key),
    async release(index = 0) {
      if (!Number.isInteger(index)) throw new TypeError("message index must be an integer");
      if (!await page.evaluate(([key, index]) => window[key].release(index), [key, index])) {
        throw new Rejected("no matching message is waiting");
      }
    },
  };
}

module.exports = { dialogs, permissions, visibility, messages };
