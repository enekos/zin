"use strict";

// The recorder alone, with no Node.js dependency: it also runs in a browser.
// recorder.js is the same code as an ES module; keep the two identical.

/**
 * Maps and nested objects as plain JSON values. A `Set` stays a set of plain
 * values, which a runner compares without regard to order.
 */
function plain(value) {
  if (value instanceof Map) {
    return Object.fromEntries([...value].map(([key, item]) => [String(key), plain(item)]));
  }
  if (value instanceof Set) return new Set([...value].map(plain));
  if (Array.isArray(value)) return value.map(plain);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, plain(item)]));
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

/** Writes the events of a running system in the format `zin trace` reads. */
class Recorder {
  /** `write` receives each line; without it the lines are kept in `lines`. */
  constructor(write) {
    this.lines = [];
    this.write = write ?? ((line) => this.lines.push(line));
  }

  event(action, args = [], { accepted = true, state } = {}) {
    const record = { action, args: plain(args), outcome: accepted ? "accepted" : "rejected" };
    if (state !== undefined) record.state = jsonable(plain(state));
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

module.exports = { Recorder, plain, jsonable };
