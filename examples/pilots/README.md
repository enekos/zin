# Conformance pilots

Run the same adapter against a known faulty implementation and its correction.
The corpus checks each verdict and prints one JSON record per run. The four fast
cases derive deterministic plans at both `transitions` and `pairs` coverage, and
each test starts from a fresh instance.

| Pilot | Observable | Defect |
| --- | --- | --- |
| Acknowledgement | acknowledged flag and acceptance | The second identical acknowledgement is refused. |
| Credit boundary | balance and acceptance | The account accepts a third credit past its capacity of two. |
| Refusal | balance and acceptance | A failed debit still updates the balance. |
| quick-lru | `cache.has("key")` | Replacing an entry in 6.0.0 drops its expiry, so a later tick leaves the key visible. Version 6.0.1 expires it. |
| Browser editor | draft, saved value, status and button refusals | An in-flight save is acknowledged after another edit and persists the latest draft. |

The cache adapter calls the pinned npm packages, controls the clock during
synchronous public API calls and leaves wall time to the test runner. There
are no timer sleeps. [Upstream change and test](https://github.com/sindresorhus/quick-lru/commit/6033bb74f4262e6784232ddf9c117a02d376e02f)

```sh
cargo build --release
export ZIN_BIN="$PWD/target/release/zin"
cd examples/pilots
npm ci
npm test
npx playwright install --only-shell chromium
npm run test:browser
```

`node corpus.mjs transitions` runs one coverage mode. The browser pilot uses the
editor fixture of the JavaScript binding, real Chromium and the same in-flight
save scenario on both variants; its JSON record is also attached to the
Playwright report. Each record lists the observed fields, mismatches, planned
and executed calls, fresh setups, time to first divergence and total replay
milliseconds. Executed calls include reproduction attempts. `null` means no
divergence.

## Measured comparison

Transition tours find three of the four faults and adjacent-call pairs find all
four; every corrected implementation passes both. Calls and setups are for the
corrected implementation, which runs the whole plan:

| Corrected implementation | Transitions: calls / setups | Pairs: calls / setups |
| --- | ---: | ---: |
| Acknowledgement | 1 / 1 | 2 / 1 |
| Credit limit | 6 / 1 | 13 / 1 |
| Mutation on refusal | 6 / 1 | 13 / 1 |
| quick-lru expiry | 4 / 1 | 9 / 1 |
| **Total** | **17 / 4** | **37 / 4** |

Pairs are chained inside tours, so every plan above is a single tour from one
setup. The faulty acknowledgement passes its one-call transition tour; the pair
`ack, ack` catches it at the second call.

In the Chromium scenario the corrected editor completes six calls and the
faulty editor diverges at the fourth (`server_ok`: "saved is 2, but the
specification says 1"). These counts describe this corpus; the JSON records
give timings for the machine that runs it.
