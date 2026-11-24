// Install before the adapter navigates or registers gates. Playwright consults
// later page routes first, so requests handled by a gate never reach this one.
async function hermetic(page, options = {}) {
  if (!page) throw new Error("hermetic replay needs the page fixture");
  const seed = options.seed ?? 1;
  if (!Number.isSafeInteger(seed)) throw new Error("hermetic seed must be an integer");
  let unexpected;
  const guard = async route => {
    const request = route.request();
    if (!["fetch", "xhr", "eventsource"].includes(request.resourceType())) return route.fallback();
    unexpected ??= `${request.method()} ${request.url()}`;
    await route.abort("blockedbyclient");
  };
  await page.addInitScript(initial => {
    let value = initial >>> 0;
    Math.random = () => {
      value = (Math.imul(value, 1664525) + 1013904223) >>> 0;
      return value / 2 ** 32;
    };
    const sheet = new CSSStyleSheet();
    sheet.replaceSync("*, *::before, *::after { animation: none !important; transition: none !important; }");
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  }, seed);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.route("**/*", guard);
  return { async dispose() {
    await page.unroute("**/*", guard);
    if (unexpected) throw new Error(`ungated API request: ${unexpected}`);
  } };
}

module.exports = { hermetic };
