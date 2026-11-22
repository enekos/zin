# Conformance pilots

Run the same adapter against a known faulty implementation and its correction. The corpus checks the verdict as well as printing one JSON record per run. The four fast cases derive deterministic plans at both `transitions` and `pairs` coverage. Each begins from a fresh instance.

| Pilot | Observable | Defect |
| --- | --- | --- |
| Acknowledgement | acknowledged flag and acceptance | The second identical acknowledgement is refused. |
| Credit boundary | balance and acceptance | The account accepts a third credit past its capacity of two. |
| Refusal | balance and acceptance | A failed debit still updates the balance. |
| quick-lru | `cache.has("key")` | Replacing an entry in 6.0.0 drops its expiry. A subsequent tick leaves the key visible. Version 6.0.1 expires it. |
| Browser editor | draft, saved value, status and button refusals | An in-flight save receives an acknowledgement after another edit and persists the latest draft. |

The cache adapter calls the actual pinned npm packages, controls the clock during synchronous public API calls and leaves wall time available to the test runner. There are no timer sleeps. [Upstream change and test](https://github.com/sindresorhus/quick-lru/commit/6033bb74f4262e6784232ddf9c117a02d376e02f)

```sh
cargo build --release --target-dir target
export ZIN_BIN="$PWD/target/release/zin"
cd examples/pilots
npm ci
npm test
npx playwright install --only-shell chromium
npm run test:browser
```

`node corpus.mjs transitions` runs one coverage mode. The browser uses the existing editor fixture, real Chromium and the same in-flight save scenario on both variants. Its JSON record is also attached to the Playwright report. Each record lists actual observed fields, mismatches, planned and executed calls, fresh setups, time to first divergence and total replay milliseconds. Reproduction attempts count toward replay cost. Time starts after plan derivation and ends after replay. `null` means no divergence. Use the times for local comparisons.

### Measured comparison

On the original four-case corpus, full transition tours find three faults and all corrected implementations pass. Adjacent-call coverage finds all four. The acknowledgement adds just one planned call. The other fixtures show why to keep both modes available.

| Fixed implementation | Transition calls / setups | Adjacent-pair calls / setups |
| --- | ---: | ---: |
| Acknowledgement | 1 / 1 | 2 / 1 |
| Credit limit | 6 / 1 | 31 / 10 |
| Mutation on refusal | 6 / 1 | 31 / 10 |
| quick-lru expiry | 4 / 1 | 18 / 7 |

On the separate Chromium scenario, the corrected editor completes six calls and the faulty editor diverges on the fourth (`server_ok`). These counts describe this corpus. The JSON records capture fresh timings on the machine running it.

## Use the installed JVM binding

The Maven consumer in `junit/` drives the JDK's `ByteBuffer`. It observes the position and checks mark validity through the public `reset` operation. Run from the repository root with `ZIN_BIN` set as above.

```sh
mvn -f bindings/junit/pom.xml install
mvn -f examples/pilots/junit/pom.xml test
mvn -f examples/pilots/junit/pom.xml test "-Dzin.case=Rewind discards"
```

The filter selects the named scenario. The binding's own tests replay a committed plan against a real `ArrayBlockingQueue`, then deliberately give the queue excess capacity and assert that JUnit reports a divergence.
