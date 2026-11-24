# Checking a web interface

A page in a browser is one more implementation: it accepts some actions,
refuses others, and shows a state. `zin-verify/playwright` replays a
specification's derived plan against the real page, in Playwright Test.

What that shows: in every state the model can reach, the page allows exactly
the actions the model allows, and shows what the model says it should. It is
exhaustive over the model, not over the code behind the page, and it says
nothing about layout or appearance.

## Model the environment too

A user interface is rarely wrong about one click. It is wrong about order:
the answer that arrives after the user moved on, the second click before the
first one finished. So the server's behavior goes into the specification as
actions, next to the user's:

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

The checker explores every interleaving of the two, and the plan contains
each one as an ordinary sequence of steps. In the test, the adapter holds the
request and answers it when the plan says `server_ok`, so a race is replayed
deterministically, with no sleeping and no luck. The full specification is
[`examples/editor.zin`](../examples/editor.zin).

## The adapter

`zin init playwright <spec>` prints this skeleton for your specification, and
`zin init testing-library <spec>` the one for a component test.

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

Each scenario and tour is its own Playwright test, so workers, shards,
`--grep`, retries, traces and the HTML report work as usual. Every step is a
`test.step`: the trace viewer shows `increase`, `save`, `reject save`,
`server_ok` with the page at each one.

The factory receives the fixtures named in `fixtures` (default `["page"]`)
and the test info. Pass your own `test` when you extended it:
`verify(spec, factory, { test, fixtures: ["page", "account"] })`.

After a failure the factory runs a second time in the same test, to replay
the shortest path to the failing state. It must start the application from
scratch (a `page.goto` does that for state kept in memory) and `teardown`
must undo what it set up on the page.

### How the page says no

`click(locator)` and `fill(locator, value)` refuse the action when a user
could not do it: the element is missing, hidden, disabled or read-only. They
never wait, and anything else (two elements match, the click times out) is an
error, not a refusal.

A selector that matches nothing therefore looks like a refusal. That cannot
pass unnoticed: with the default coverage every action is accepted somewhere
in the plan, and there the test fails with "the implementation rejected
increase, but the specification allows it".

If the page refuses in another way (the button works but an error appears and
nothing changes), throw `Rejected` yourself:

```js
import { Rejected } from "zin-verify/playwright";

close: async () => {
  await page.getByRole("button", { name: "Close" }).click();
  if (await page.getByRole("alert").isVisible()) throw new Rejected();
},
```

### When the page catches up

A page renders after the action returns. After each step the binding keeps
calling `observe` until the state matches the model, for up to
`observeTimeout` (default 5000 ms), and reports the last observation if it
never does. A step that matches at once costs one observation.

Because `click` does not wait, the previous step must have left the page at
rest. Observing the fields that decide what is enabled does that. For work
the page does out of sight, give the adapter a `settle()`; it is awaited after
every action.

### The server as actions

`gate(page, url)` holds every request to `url` (also on a browser context).

| Call | Does |
| --- | --- |
| `server.request(action)` | performs an action that sends a request, and returns once the gate holds it; an accepted action that sends nothing is an error |
| `server.fulfill(response)` | answers the oldest held request; `response` is what `route.fulfill` takes, or a function of the request |
| `server.abort()` / `server.continue()` | fails it at the network level / lets it through to the real server |
| `server.pending`, `server.requests` | what is held |
| `server.dispose()` | stops holding; call it from `teardown` |

With nothing held, `fulfill`, `abort` and `continue` refuse the action, which
is what the model says about `server_ok` when nothing is being saved. Each
takes a last argument `match` (a predicate on the request) to answer a
particular request, for models in which answers overtake each other.

## Components, without a browser

The same specification checks a component in jsdom with the plain `verify`,
at a fraction of the cost: the whole plan for the example runs in well under
a second. Refusal is a disabled control, and the server is a stubbed `fetch`
whose promises the adapter resolves:

```tsx
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
ships. [`examples/react-editor`](../examples/react-editor) has both against
one React application.

## Sessions nobody scripted

A plan visits what the model says is reachable. The other direction is to
let the page run (a random walk, an end-to-end suite you already have, a
staging session) and ask afterwards whether what happened is a behavior of the
specification. `zin-verify/recorder` is the `Recorder` with no Node.js
dependency, so the page itself can keep the log:

```tsx
import { Recorder } from "zin-verify/recorder";

const session = new Recorder();          // or new Recorder((line) => send(line))
<Editor onEvent={(action) => session.event(action)} />
```

Record an action where it takes effect: the click handler for the user's
actions, the resolved request for the server's. Then check the log:

```sh
zin trace editor.zin session.jsonl
```

```
violation: trace line 10: the implementation accepted save, but the
specification rejects it in state draft=2, saved=1, sent=2, status=saving
```

`e2e/session.spec.ts` in the example clicks 150 random enabled buttons against
a server that answers late and fails one time in four, reads
`session.text()` from the page and runs `zin trace` on it. Pass
`{ state: {...} }` to `event` to have the state compared as well; see
[checking recorded executions](conformance.md#checking-recorded-executions).

## Cost

A step is a click and a few reads: a few milliseconds against a local page.
The plan has roughly `states × calls` steps, so:

- Keep ranges small. Three values of a quantity find the same defects as
  thirty.
- A tour of 200 steps may not fit Playwright's 30 s test timeout on a slow
  application. `verify(spec, factory, { test, maxSteps: 50 })` makes more,
  shorter tests, which also spread better across workers.
- `cover: "states"` for pull requests and the default nightly is a common
  split.
- Commit the plan (`zin plan editor.zin --out editor.plan.json`) so that test
  workers need neither the binary nor a second of start-up each.

## What it does not show

- Anything the model omits. A page that behaves differently for a quantity of
  7 when the model stops at 2 is not exercised there.
- Appearance. Use screenshots or visual review for that; `observe` can read
  whatever a user could read, but it reads text and attributes.
- Timing. The model orders events; it does not say how long they take.

## Binding fields and controls

`bind({ url, requests, actions, fields })` opens a page for each test, installs an
optional HTTP gate before navigation, and disposes it afterwards. Buttons match an
exact accessible name. `button("Save", { request: true })` waits for the request
and `response(204)` answers it. Use a function `({ page, server }, args) => …`
for any other action, including translating model symbols into input strings.
Field readers such as `testId("draft", Number)` and `role("status")` also declare
the projection for `zin plan --observe`. Consecutive refusals of these controls
can share an enabledness check and observation. Custom actions replay individually.

Generate your spec's TypeScript contract with `zin types editor.zin > editor.d.ts`.
Use its `Adapter` with `satisfies` for a function adapter, or check a declarative
map with `satisfies Binding<Actions, State>` from `zin-verify/playwright`.

Use `hermetic: { seed: 1 }` to block service workers for the generated tests
and fail a test on an ungated fetch, XHR or event stream. The page gets seeded
randomness on navigation, reduced motion and disabled CSS animations. Install
your routes in the adapter factory. Use `coverage: true` and configure
`zin-verify/playwright-reporter` as a Playwright reporter to count distinct
attempted model operations, failures and observed fields. On shards use the
blob reporter, collect the blobs and run
`npx playwright merge-reports --reporter=zin-verify/playwright-reporter ./all-blob-reports`.

## Environment actions

| Source | Browser binding |
| --- | --- |
| HTTP | `gate(page, url)` holds requests for `fulfill`, `abort` or `continue`. |
| WebSocket | Install `await socketFrames(page, url)` before connecting. Incoming frames wait for `release(predicate?)`. Supply a synchronous reply function to use a mock server. The route lives for the page's lifetime. |
| SSE | Point an EventSource at `(await eventStream()).url`. After it opens, `hold(data, event?)` queues a native SSE event for current subscribers and `release(predicate?)` sends it. Dispose the stream in teardown. |
| Time | Install `await clock(page)` before application timers are created. Bind model actions to `advance(milliseconds)`. |
| History | `navigation(page)` offers `back`, `forward` and `reload`. Back and forward check the browser Navigation API and refuse at history boundaries. |
| Network | `connectivity(page).offline()` and `.online()` switch the browser context's connectivity. Redundant changes refuse. |
| Actors | `await actors(browser, values)` isolates each actor in a context. Pass a context to model tabs sharing storage. Use `page(value)` and `dispose()`. |

`Channel<T>` provides the common queue. `hold(value)` adds a delivery and
`take(predicate?)` refuses when no matching item is waiting.
The debounce, tabs and frames pilots replay the same scenarios against correct
and deliberately faulty implementations in Chromium.

## State probes and transient changes

Pass `onState(state, page, { id, testInfo })` to inspect each canonical state
on its first matched visit within a generated test. The editor attaches a
screenshot and runs axe in this hook. A hook can also assert focus, layout or a
performance budget. Each worker owns its page and its Playwright attachments.

Pass a self-contained synchronous `monitor: () => ({ status: … })` to sample
DOM fields at MutationObserver delivery boundaries. During an action either
the pre-state or post-state may match. Between actions the current state must
match. The reader runs in the document and must return at least one field.
Monitoring is scoped to that document. Changes coalesced into one observer
delivery produce one sample.
