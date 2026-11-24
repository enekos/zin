// Declarative readers and actions share the same locators used during replay.
function button(name, options = {}) {
  return { kind: "button", name, request: options.request };
}

function response(status) {
  return { kind: "response", status };
}

function oneOf(...values) {
  return text => {
    if (!values.includes(text)) throw new Error(`expected one of ${values.join(", ")}, read ${text}`);
    return text;
  };
}

function role(name, parse = String) {
  return async ({ page }) => parse(await page.getByRole(name).innerText());
}

function testId(id, parse = String) {
  return async ({ page }) => parse(await page.getByTestId(id).innerText());
}

// CSS text fields can be sampled in a single document turn. Keep arbitrary
// function readers for controls or observations outside the document.
function text(selector, parse = String) {
  const read = async ({ page }) => parse(await page.locator(selector).textContent());
  return Object.assign(read, { selector, parse });
}

function observation(fields) {
  const entries = Object.entries(fields);
  const grouped = entries.filter(([, read]) => typeof read.selector === "string");
  const individual = entries.filter(([, read]) => typeof read.selector !== "string");
  const selectors = grouped.map(([, read]) => read.selector);
  return async scope => {
    const result = await Promise.all(individual.map(async ([key, read]) => [key, await read(scope)]));
    if (grouped.length) {
      const snapshot = await scope.page.evaluate(selectors => selectors.map(selector => {
        const elements = document.querySelectorAll(selector);
        if (elements.length > 1) throw new Error(`ambiguous text field: ${selector}`);
        return elements[0]?.textContent ?? null;
      }), selectors);
      for (const [index, [key, read]] of grouped.entries()) {
        const raw = snapshot[index];
        result.push([key, raw === null ? await read(scope) : read.parse(raw)]);
      }
    }
    return Object.fromEntries(result);
  };
}

function binding({ gate, click }) {
  return function bind({ url, requests, actions, fields, restore }) {
    const observe = observation(fields);
    const factory = async ({ page }) => {
      const server = requests && await gate(page, requests);
      const scope = { page, server: server || undefined };
      const locator = (name, enabled = false) => page.getByRole("button", {
        name, exact: true, ...(enabled && { disabled: false }),
      });
      const dispose = async () => { if (server) await server.dispose(); };
      try {
        if (url !== undefined) await page.goto(url);
        const functions = Object.fromEntries(Object.entries(actions).map(([key, action]) => {
          if (typeof action === "function") return [key, args => action(scope, args)];
          if ((action.kind === "response" || action.request) && !server) {
            throw new Error(`binding action '${key}' needs a requests URL`);
          }
          if (action.kind === "response") return [key, () => server.fulfill({ status: action.status })];
          if (action.kind !== "button") throw new Error(`unknown binding for '${key}'`);
          const press = () => click(locator(action.name));
          return [key, action.request ? () => server.request(press) : press];
        }));
        return {
          actions: functions,
          observe: () => observe(scope),
          restore: restore && (state => restore(state, scope)),
          async refusals(calls) {
            let buttons;
            for (const { action } of calls) {
              const descriptor = actions[action];
              if (descriptor?.kind === "button") {
                const current = locator(descriptor.name, true);
                buttons = buttons ? buttons.or(current) : current;
              } else if (descriptor?.kind !== "response" || server.pending) return false;
            }
            // The role selector handles accessible names and disabled ancestors.
            return !buttons || (await buttons.count()) === 0;
          },
          teardown: dispose,
        };
      } catch (error) {
        await dispose();
        throw error;
      }
    };
    factory.fields = Object.keys(fields);
    return factory;
  };
}

module.exports = { binding, button, response, role, testId, text, oneOf };
