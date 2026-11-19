# A React application checked against a specification

`src/Editor.tsx` is a small form: edit a quantity, save it, keep editing
while the save is in flight. [`../editor.zin`](../editor.zin) says what it
must do, including what happens when the server answers late or fails. The
same specification drives two test suites, and neither contains a written
test case:

| Suite | Runs | Implementation under test |
| --- | --- | --- |
| `tests/Editor.test.tsx` | Vitest, jsdom, Testing Library | the component, `fetch` stubbed |
| `e2e/editor.spec.ts` | Playwright, Chromium | the application served by Vite, requests held by a `gate` |

Both replay the derived plan: 36 states, every transition, every refused
action in every state, with the server's answers ordered by the plan.

```sh
cargo install zin-spec        # or set ZIN_BIN to a built zin
npm install
npx playwright install chromium
npm test
```

To see a failure, let the user save twice: in `src/Editor.tsx` change the Save
button to `disabled={draft === saved}`. Both suites then report

```
the implementation accepted save, but the specification rejects it
steps taken:
  1. reject save
  2. reject server_ok
  3. increase
  4. save
  5. reject save
```

and the Playwright trace (`npx playwright show-trace`) shows the page at each
of those steps. See [checking a web interface](../../docs/browser.md).
