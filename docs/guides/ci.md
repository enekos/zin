---
title: Continuous integration
parent: Guides
nav_order: 3
---

# Continuous integration
{: .no_toc }

Three gates, from cheapest to most thorough: the specification is valid and its properties hold, every
rule in it is pinned down, and the implementation conforms.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install the binary

The release binaries are the fastest way onto a CI machine:

```yaml
- name: Install zin
  run: |
    curl -fsSL https://github.com/enekos/zin/releases/download/v0.5.0/zin-v0.5.0-x86_64-unknown-linux-gnu.tar.gz \
      | tar xz --strip-components=1 -C /usr/local/bin zin-v0.5.0-x86_64-unknown-linux-gnu/zin
```

Each release attaches `zin-<version>-<target>.tar.gz` for `x86_64` and `aarch64`, Linux and macOS,
plus `SHA256SUMS`. `cargo install zin-spec --locked` also works, at the cost of a build.

Test jobs that only replay a [committed plan](#derive-at-test-time-or-commit-the-plan) do not need the
binary at all.

## The three gates

```yaml
- run: zin check specs/*.zin
- run: zin audit specs/loan.zin
- run: pytest            # or npm test, go test ./..., cargo test, mvn test
```

| Gate | Fails when | Exit codes |
| --- | --- | --- |
| `zin check` | the specification does not parse, a step leaves a declared range, or a property fails | `0` ok, `1` wrong, `2` unusable |
| `zin audit` | a mutant survives | `0` none survived, `1` some did |
| your tests | the implementation diverges | your framework's |

`zin check` takes several files and checks each. `zin audit` takes one specification per run.

## Machine-readable results

`zin check --format json` prints one result per file in the
[`check/1`](../plan-format.md#check1) format, with stable diagnostic codes and spans, ready for
annotations:

```console
$ zin check --format json loan.zin bad.zin
{"zin":"check/1","results":[{"file":"loan.zin","ok":true,"system":"Library loan","states":15, ...},
 {"file":"bad.zin","ok":false,"error":{"kind":"violation","code":"violation","line":23, ...}}]}
```

A small step turns errors into GitHub annotations:

```yaml
- name: Check specifications
  run: |
    zin check --format json specs/*.zin > check.json || status=$?
    jq -r '.results[] | select(.ok | not) | .error
      | "::error file=\(.span.file // ""),line=\(.line // 1)::\(.message | split("\n")[0])"' check.json
    exit ${status:-0}
```

## Derive at test time, or commit the plan

Every binding looks for the plan in the same order: a plan you pass explicitly, a sibling
`<spec>.plan.json`, then the `zin` binary (`ZIN_BIN`, then `PATH`).

| | Derive at test time | Commit the plan |
| --- | --- | --- |
| Test machines need | the `zin` binary | only the binding |
| The specification is re-checked | on every test run | when you regenerate |
| A changed specification | is picked up automatically | fails loading with the command to run |

Committing is the usual choice for slow or large plans, and for test machines you do not control. Keep
it honest with a job that regenerates and diffs:

```yaml
- run: |
    for spec in specs/*.zin; do zin plan "$spec" --out "${spec%.zin}.plan.json"; done
    git diff --exit-code -- 'specs/*.plan.json'
```

A sibling plan is used only when it is fresh for the specification and every file it includes, and was
derived with zin's default for every planning option the test leaves out. A plan with weaker coverage
(say `--cover scenarios`) is never picked up silently; naming it explicitly is the opt-in.

## Coverage on a budget

For a slow implementation (a browser, a database), split the work:

```yaml
# on every pull request
- run: npx playwright test           # verify(spec, factory, { test, cover: "states" })

# nightly
- run: ZIN_COVER=transitions npx playwright test
```

Pass the option from an environment variable in the test file
(`cover: process.env.ZIN_COVER ?? "states"`). Keep ranges small: three values of a quantity find the
same defects as thirty.

## Coverage reports

- **pytest:** `pytest --zin-coverage` reports, per suite, the unique (model state, call) operations
  attempted out of those planned, the ones that failed, and the fields `observe` compared.
- **Vitest:** add `zin-verify/vitest-reporter` to `reporters` for the same numbers.

See [Python](../bindings/python.md) and [JavaScript](../bindings/javascript.md).

## A complete workflow

```yaml
name: specs
on: [push, pull_request]

jobs:
  specs:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install zin
        run: |
          curl -fsSL https://github.com/enekos/zin/releases/download/v0.5.0/zin-v0.5.0-x86_64-unknown-linux-gnu.tar.gz \
            | tar xz --strip-components=1 -C /usr/local/bin zin-v0.5.0-x86_64-unknown-linux-gnu/zin
      - run: zin check specs/*.zin
      - run: for spec in specs/*.zin; do zin audit "$spec"; done
      - uses: actions/setup-python@v5
        with: { python-version: "3.12" }
      - run: pip install pytest ./vendor/zin/bindings/python
      - run: pytest --zin-coverage
```
