// The recorder alone, with no Node.js dependency: it also runs in a browser.
// recorder.cjs is the same code for CommonJS; keep the two identical.

/** A value that was observed before it settled: a Promise or any thenable. */
class Unawaited extends TypeError {
  constructor(path) {
    super(`a promise for ${path}; await it`);
    this.name = "Unawaited";
    this.path = path;
  }
}

const thenable = (value) =>
  value !== null && (typeof value === "object" || typeof value === "function") && typeof value.then === "function";

/**
 * Maps and nested objects as plain JSON values. A `Set` stays a set of plain
 * values, which a runner compares without regard to order. Other objects keep
 * their own enumerable properties. A promise throws `Unawaited`, naming `path`.
 */
function plain(value, path = "the value") {
  if (thenable(value)) throw new Unawaited(path);
  if (value instanceof Map) {
    return Object.fromEntries([...value].map(([key, item]) => [String(key), plain(item, `${path}.${String(key)}`)]));
  }
  if (value instanceof Set) return new Set([...value].map((item) => plain(item, `${path} member`)));
  if (Array.isArray(value)) return value.map((item, index) => plain(item, `${path}[${index}]`));
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, plain(item, `${path}.${key}`)]));
  }
  return value === undefined ? null : value;
}

/** A plain value as JSON: sets become arrays in a stable order. */
function jsonable(value) {
  if (value instanceof Set) {
    return [...value].map(jsonable).sort((a, b) => (JSON.stringify(a) < JSON.stringify(b) ? -1 : 1));
  }
  if (Array.isArray(value)) return value.map(jsonable);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, jsonable(item)]));
  }
  return value;
}

/** `value` as plain JSON; a promise in it fails the event that recorded it. */
function recorded(action, value, path) {
  try {
    return plain(value, path);
  } catch (error) {
    if (error instanceof Unawaited) throw new TypeError(`the ${action} event got ${error.message}`);
    throw error;
  }
}

/** Writes the events of a running system in the format `zin trace` reads. */
class Recorder {
  /** `write` receives each line; without it the lines are kept in `lines`. */
  constructor(write) {
    this.lines = [];
    this.write = write ?? ((line) => this.lines.push(line));
  }

  event(action, args = [], { accepted = true, state } = {}) {
    const record = { action, args: recorded(action, args, "args"), outcome: accepted ? "accepted" : "rejected" };
    if (state !== undefined) record.state = jsonable(recorded(action, state, "state"));
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

export { Recorder, Unawaited, plain, jsonable };
