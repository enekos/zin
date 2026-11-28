# Runner contract cases

Every runner (Python, JavaScript, Go, JUnit) runs these files in its own test suite.
They pin the behavior that [docs/plan-format.md](../../../docs/plan-format.md#what-a-binding-must-do)
specifies, so the runners cannot drift apart. A new runner passes them first.

| File | What it pins |
| --- | --- |
| `cases.json` `load` | Plan loading: fresh and stale plans, the combined fingerprint of included files, option compatibility, `.json` paths and malformed calls. Runs with a binary path that does not exist, so a case that needs `zin plan` reports it. `expect.errors` are fragments the error message must contain. |
| `cases.json` `replay` | Replay and reporting against a scripted adapter. `expect.report` is the exact divergence text; `expect.raises` is an implementation error that must propagate unchanged; `expect.factories` counts the fresh implementations built. |
| `observations.json` | Matching an observation against a state: complete maps, records and variants, unknown keys and fields, strict types. |
| `native-observations.json` | Normalizing native values first. `{"$enum": name}` stands for the runner's enum-like value (a Python `Enum` member, a Go named string type, a Java enum constant, a plain string in JavaScript); `{"$map": [[key, value], ...]}` for a map with non-string keys. A case's optional `fields` gives the plan's field types, which the runner passes on as in a replay. |

Option names are the plan's keys (`max_steps`); each runner maps them to its own
spelling (`maxSteps`, `WithMaxSteps`, `Options.maxSteps`).

## The scripted adapter

`cases.json` `adapter` gives the default; a case's `adapter` overrides its keys.

- `initial`: the implementation's starting state.
- `rules`: for each action, the state it `requires` and the state it `set`s
  when accepted. A call whose requirement fails is rejected.
- `observe`: whether `observe` returns the whole state.
- `overrides`: checked before the rules, first match wins. `instance` (the
  1-based count of implementations built), `call` (the 1-based call number on
  that implementation) and `action` narrow the match when present. `outcome`
  is `accept` or `reject` (the state is unchanged) or `raise` (an error with
  `message` that is not a rejection).

## Regenerating

The plans are committed so the cases run without the binary:

```sh
cd bindings/fixtures/contract
zin plan latch.zin --out latch.plan.json
zin plan latch.zin --cover states --out states.plan.json
zin plan composed/main.zin --out composed/main.plan.json
cp composed/main.plan.json drifted/main.plan.json
zin plan weaker/latch.zin --cover states --out weaker/latch.plan.json
```

`stale.plan.json` was derived from a different `latch.zin`, `arity.plan.json` is
`latch.plan.json` with an extra argument on its first call, and `weaker/` is a
sibling plan with weaker coverage than the default.
