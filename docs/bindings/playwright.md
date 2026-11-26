---
title: Playwright and components
parent: Bindings
nav_order: 3
---

# Playwright and components
{: .no_toc }

A page in a browser is one more implementation: it accepts some actions,
refuses others, and shows a state. `zin-verify/playwright` replays the plan
against the real page in Playwright Test, and the plain `verify` replays it
against a component in jsdom.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

```sh
npm install --save-dev zin-verify @playwright/test
```

`@playwright/test` is an optional peer dependency of `zin-verify`. `zin init
playwright editor.zin` prints a skeleton; `zin init testing-library editor.zin`
prints the component version.

## Model the environment too

An interface is rarely wrong about one click. It is wrong about order: the
answer that arrives after the user moved on, the second click before the
first finished. So the server goes into the specification as actions, next to
the user's:

```zin
action save                 # the user
  requires status is not saving
  requires draft is not saved
  sent becomes draft
  status becomes saving

fair action server_ok       # the environment
  requires status is saving
  saved becomes sent
  status becomes idle

action server_fails
  requires status is saving
  sent becomes saved
  status becomes failed
```

The checker explores every interleaving, and the plan contains each one as an
ordinary sequence of steps. In the test, the adapter holds the request and
answers it when the plan says `server_ok`, so a race replays
deterministically: no sleeping, no luck. The full specification is
[`examples/editor.zin`](https://github.com/enekos/zin/blob/main/examples/editor.zin).

## The adapter

```js
import { test } from "@playwright/test";
import { verify, click, gate } from "zin-verify/playwright";

verify(new URL("./editor.zin", import.meta.url), async ({ page }) => {
  const server = await gate(page, "**/api/quantity");
  await page.goto("/");
  const button = (name) => page.getByRole("button", { name });
  const number = async (id) => Number(await page.getByTestId(id).textContent());
  return {
    actions: {
      increase: () => click(button("More")),
      decrease: () => click(button("Less")),
      save: () => server.request(() => click(button("Save"))),
      server_ok: () => server.fulfill({ json: { ok: true } }),
      server_fails: () => server.fulfill({ status: 500 }),
    },
    observe: async () => ({
      draft: await number("draft"),
      saved: await number("saved"),
      status: await page.getByRole("status").textContent(),
    }),
    teardown: () => server.dispose(),
  };
}, { test });
```

The adapter has the same shape as in [JavaScript](javascript.md#the-factory-and-the-adapter):
`actions`, `rejects`, `observe`, `settle`, `teardown`.

Each scenario and tour is its own Playwright test, so workers, shards,
`--grep`, retries, traces and the HTML report work as usual. Every step is a
`test.step`: the trace viewer shows `increase`, `save`, `reject save`,
`server_ok`, with the page at each one.

### verify

```ts
verify(spec, factory, options?): Plan
```

`factory(fixtures, testInfo)` receives the fixtures named in `options.fixtures`
and Playwright's `TestInfo`, and returns the adapter.

| Option | Default | Meaning |
| --- | --- | --- |
| `test` | `test` from `@playwright/test` | yours, when you extended it with fixtures |
| `fixtures` | `["page"]` | the fixtures the factory receives (identifiers only) |
| `name` | `"<system> conforms to <file>"` | the `test.describe` name |
| `observeTimeout` | `5000` ms | how long an observation may take to match |
| `observeInterval` | `20` ms | pause between observations |
| `cover`, `maxSteps`, `stepBudget`, `pairLimit`, `component`, `plan`, `bin` | | [planning options](javascript.md#planning-options-planoptions) |

```js
import { test as base } from "@playwright/test";

const test = base.extend({ account: async ({}, use) => use(await createAccount()) });

verify(spec, async ({ page, account }) => { /* ... */ }, { test, fixtures: ["page", "account"] });
```

### Starting from scratch, twice

The factory runs before every plan test, and after a failure it runs a second
time **in the same test** to replay the shortest path to the failing state.
It must start the application from scratch: a `page.goto` does that for state
kept in memory; state kept on a server needs a reset there. `teardown` must
undo whatever the factory set up on the page (`server.dispose()`, unrouting).

## How the page says no

| Helper | Does |
| --- | --- |
| `click(locator, options?)` | throws `Rejected` when the element is missing, hidden or disabled; otherwise `locator.click(options)` |
| `fill(locator, value, options?)` | throws `Rejected` when the field is missing, hidden, disabled or read-only; otherwise `locator.fill(String(value), options)` |
| `actionable(locator)` | `true` when the element is visible and enabled; never waits |

They never wait. Anything else (two elements match, the click times out) is an
error, not a refusal.

A selector that matches nothing therefore looks like a refusal. That cannot
pass unnoticed: with the default coverage every action is accepted somewhere
in the plan, and there the test fails with `the implementation rejected
increase, but the specification allows it`.

When the page refuses in another way (the button works, but an error appears
and nothing changes), throw `Rejected` yourself:

```js
import { Rejected } from "zin-verify/playwright";

close: async () => {
  await page.getByRole("button", { name: "Close" }).click();
  if (await page.getByRole("alert").isVisible()) throw new Rejected();
},
```

## When the page catches up

A page renders after the action returns. After each step the binding keeps
calling `observe` until it matches the model, for up to `observeTimeout`
(default 5000 ms here, 0 in the plain binding), and reports the last
observation if it never does. A step that matches at once costs one
observation.

Because `click` does not wait, the previous step must have left the page at
rest. Observing the fields that decide what is enabled does that. For work the
page does out of sight, give the adapter a `settle()`: it is awaited after every
action.

## The server as actions: gate

```ts
gate(target: Page | BrowserContext, url: string | RegExp | ((url: URL) => boolean), options?: { timeout?: number }): Promise<Gate>
```

`gate` routes `url` on a page or a browser context and holds every matching
request until the plan decides its outcome.

| Member | Does |
| --- | --- |
| `server.request(action)` | performs an action that sends a request, and returns once the gate holds it. A refused action stays refused; an accepted one that sends nothing fails with `no request to <url> arrived within <timeout> ms` (`options.timeout`, default 5000) |
| `server.fulfill(response?, match?)` | answers the oldest held request; `response` is what `route.fulfill` takes, or a (possibly async) function of the request |
| `server.abort(errorCode?, match?)` | fails it at the network level |
| `server.continue(overrides?, match?)` | lets it through to the real server |
| `server.pending` | how many requests are held |
| `server.requests` | the held `Request`s, oldest first |
| `server.dispose()` | stops holding and aborts what is held; call it from `teardown` |

With nothing held, `fulfill`, `abort` and `continue` throw `Rejected`, which is
what the model says about `server_ok` when nothing is being saved. `match`, a
predicate on the request, answers a particular one, for models in which
answers overtake each other:

```js
server_ok_for: ({ id }) => server.fulfill({ json: { ok: true } }, (request) => request.url().endsWith(`/${id}`)),
```

## Components, without a browser

The same specification checks a component in jsdom with the plain `verify`, at
a fraction of the cost: the whole plan for the editor runs in well under a
second. Refusal is a disabled control, and the server is a stubbed `fetch`
whose promises the adapter resolves:

```tsx
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { describe, test, vi } from "vitest";
import { Rejected, verify } from "zin-verify";

function press(name: string) {
  const button = screen.getByRole<HTMLButtonElement>("button", { name });
  if (button.disabled) throw new Rejected();
  fireEvent.click(button);
}

verify(join(import.meta.dirname, "editor.zin"), () => {
  const waiting: ((response: Response) => void)[] = [];
  vi.stubGlobal("fetch", () => new Promise<Response>((resolve) => waiting.push(resolve)));
  render(<Editor />);

  async function answer(status: number) {
    const resolve = waiting.shift();
    if (!resolve) throw new Rejected();
    await act(async () => resolve(new Response(null, { status })));
  }

  return {
    actions: { save: () => press("Save"), server_ok: () => answer(204), server_fails: () => answer(500) },
    observe: () => ({ status: screen.getByRole("status").textContent }),
    teardown: () => { cleanup(); vi.unstubAllGlobals(); },
  };
}, { describe, test });
```

Under Vitest with jsdom, give the specification as a path: Vite resolves
`new URL("./editor.zin", import.meta.url)` to an `http:` address there.

Run the component suite on every change and the browser suite where the
integration matters: routing, real requests, focus, what the bundle actually
ships. [`examples/react-editor`](https://github.com/enekos/zin/tree/main/examples/react-editor)
has both against one React application.

## Sessions nobody scripted

A plan visits what the model says is reachable. The other direction is to let
the page run (a random walk, an end-to-end suite you already have, a staging
session) and ask afterwards whether what happened is a behavior of the
specification. `zin-verify/recorder` has no Node.js dependency, so the page
itself can keep the log:

```tsx
import { Recorder } from "zin-verify/recorder";

const session = new Recorder();          // or new Recorder((line) => send(line))
<Editor onEvent={(action) => session.event(action)} />
```

Record an action where it takes effect: the click handler for the user's
actions, the resolved request for the server's. Then:

```console
$ zin trace editor.zin session.jsonl
violation: trace line 10: the implementation accepted save, but the specification rejects it in state draft=2, saved=1, sent=2, status=saving
```

`e2e/session.spec.ts` in the example clicks 150 random enabled buttons against
a server that answers late and fails one time in four, reads
`session.text()` from the page and runs `zin trace` on it. Pass
`{ state: {...} }` to `event` to have the state compared too.

## Cost

A step is a click and a few reads: a few milliseconds against a local page.
The plan has roughly `states × calls` steps, so:

- **Keep ranges small.** Three values of a quantity find the same defects as
  thirty.
- **Shorter tours.** A 200-step tour may not fit Playwright's 30 s test
  timeout on a slow application. `{ test, maxSteps: 50 }` makes more, shorter
  tests, which also spread better across workers.
- **Split coverage.** `cover: "states"` on pull requests, the default nightly.
- **Commit the plan** (`zin plan editor.zin --out editor.plan.json`) so test
  workers need neither the binary nor its start-up time.

## What it does not show

- Anything the model omits: a page that misbehaves at a quantity of 7 when the
  model stops at 2 is not exercised there.
- Appearance. `observe` reads text and attributes; use screenshots for looks.
- Timing. The model orders events; it does not say how long they take.

## Troubleshooting

| Message | Cause |
| --- | --- |
| `@playwright/test could not be loaded (...); pass its test: verify(spec, factory, { test })` | the peer dependency is missing or resolves elsewhere |
| `'my-fixture' is not a fixture name` | `fixtures` entries must be JavaScript identifiers |
| `no request to **/api/quantity arrived within 5000 ms` | an action wrapped in `server.request` was accepted but sent nothing |
| `the implementation rejected increase, but the specification allows it` | often a locator that matches nothing |
| `... (waited 5000 ms)` | the page never showed the expected state |
