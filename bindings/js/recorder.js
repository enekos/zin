// The recorder alone, with no Node.js dependency: it also runs in a browser.
// recorder.cjs is the same code for CommonJS; keep the two identical.

/** Maps and nested objects as plain JSON values. */
function plain(value) {
  if (value instanceof Map) {
    return Object.fromEntries([...value].map(([key, item]) => [String(key), plain(item)]));
  }
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, plain(item)]));
  }
  return value;
}

/** Writes the events of a running system in the format `zin trace` reads. */
class Recorder {
  /** `write` receives each line; without it the lines are kept in `lines`. */
  constructor(write) {
    this.lines = [];
    this.write = write ?? ((line) => this.lines.push(line));
  }

  event(action, args = [], { accepted = true, state } = {}) {
    const record = { action, args: plain(args), outcome: accepted ? "accepted" : "rejected" };
    if (state !== undefined) record.state = plain(state);
    this.write(`${JSON.stringify(record)}\n`);
  }

  /** Marks the start of a new run from the initial state. */
  reset() {
    this.write('{"reset":true}\n');
  }

  /** The kept lines as one log. */
  text() {
    return this.lines.join("");
  }
}

export { Recorder, plain };
