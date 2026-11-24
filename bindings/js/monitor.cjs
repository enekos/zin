// This function runs in the document. MutationObserver samples at mutation
// delivery boundaries, including changes while the runner is waiting to settle.
function install(read) {
  let allowed = [];
  let failure;
  function matches(value, expected) {
    if (value === null || typeof value !== "object") return value === expected;
    return expected != null && typeof expected === "object" &&
      Object.entries(value).every(([key, item]) => Object.hasOwn(expected, key) && matches(item, expected[key]));
  }
  function sample() {
    if (failure || !allowed.length) return;
    try {
      const actual = read();
      if (!actual || Object.getPrototypeOf(actual) !== Object.prototype || !Object.keys(actual).length) {
        throw new Error("monitor must return a nonempty object of state fields");
      }
      if (!allowed.some(state => matches(actual, state))) {
        failure = `intermediate observation ${JSON.stringify(actual)} outside pre- and post-state`;
      }
    } catch (error) { failure = `monitor: ${error.message}`; }
  }
  const observer = new MutationObserver(sample);
  observer.observe(document, { subtree: true, childList: true, characterData: true, attributes: true });
  return {
    allow(states) {
      sample();
      allowed = states;
      sample();
      return failure;
    },
    stop() { observer.disconnect(); },
  };
}

async function monitor(page, read) {
  const handle = await page.evaluateHandle(`(${install.toString()})((${read.toString()}))`);
  return {
    allow: states => handle.evaluate((observer, states) => observer.allow(states), states),
    async dispose() {
      try { await handle.evaluate(observer => observer.stop()); }
      finally { await handle.dispose(); }
    },
  };
}

module.exports = { monitor };
