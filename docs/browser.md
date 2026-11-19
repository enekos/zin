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
