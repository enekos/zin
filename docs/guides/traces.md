---
title: Checking recorded executions
parent: Guides
nav_order: 2
---

# Checking recorded executions
{: .no_toc }

A plan visits what the model says is reachable. The other direction is to let the real system run (a
service in staging, an end-to-end suite you already have, a random walk through a page) and ask
afterwards whether what happened is a behavior of the specification.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## The log

`zin trace <spec> <log>` reads JSON lines, one event per line:

```json
{"action":"borrow","args":{"m":"ana"},"outcome":"accepted","state":{"holder":"ana"}}
{"action":"reserve","args":["ben"]}
{"action":"renew","outcome":"rejected"}
{"reset":true}
```

| Key | Meaning |
| --- | --- |
| `action` | the action's name in the specification |
| `args` | by parameter name (an object) or by position (an array); omit for none |
| `outcome` | `accepted` (the default) or `rejected` |
| `state` | optional; any subset of the fields, compared after the event, with the same JSON encoding as [plans](../plan-format.md) |
| `{"reset":true}` | a new run starts from the initial state |

The log must be a **linearized** history of one instance of the system: the order of lines is the
order in which the calls took effect. If several instances run at once, write one log per instance, or
a `reset` between runs of one instance.

## Checking it

```console
$ zin trace loan.zin events.jsonl
ok Library loan: 7 events in 1 run(s) conform to the specification
```

An accepted call the model refuses:

```console
$ zin trace loan.zin events.jsonl
violation: trace line 3: the implementation accepted renew, but the specification rejects it in state holder=ana, renewals=0, waiting=[ben]
```

A state that differs from the model:

```console
$ zin trace loan.zin events.jsonl
violation: trace line 1: after borrow(ana), renewals is 1, but the specification says 0
```

`-` reads the log from standard input, so a log can be streamed from wherever it lives:

```console
$ kubectl logs deploy/library | grep '^{"action"' | zin trace loan.zin -
```

Exit codes follow the other commands: `0` conforms, `1` a violation, `2` the log or the specification
could not be used. `--symmetry on|off` controls the reduction for
[interchangeable values](../language/types.md).

## Recording from Python

`zin.Recorder` writes the format. Give it a path (appended to) or any object with `write`:

```python
import zin
from library import Book

book = Book()
with zin.Recorder("events.jsonl") as log:
    def call(name, *args):
        try:
            getattr(book, name)(*args)
            log.event(name, list(args), state={
                "holder": book.holder,
                "renewals": book.renewals,
                "waiting": set(book.waiting),
            })
        except ValueError:
            log.event(name, list(args), accepted=False)

    call("borrow", "ana")
    call("renew")
    call("reserve", "ben")
    call("renew")
    call("give_back")
    call("borrow", "ana")
    call("borrow", "ben")
```

```json
{"action":"borrow","args":["ana"],"outcome":"accepted","state":{"holder":"ana","renewals":0,"waiting":[]}}
{"action":"renew","args":[],"outcome":"accepted","state":{"holder":"ana","renewals":1,"waiting":[]}}
{"action":"reserve","args":["ben"],"outcome":"accepted","state":{"holder":"ana","renewals":1,"waiting":["ben"]}}
{"action":"renew","args":[],"outcome":"rejected"}
{"action":"give_back","args":[],"outcome":"accepted","state":{"holder":null,"renewals":0,"waiting":["ben"]}}
{"action":"borrow","args":["ana"],"outcome":"rejected"}
{"action":"borrow","args":["ben"],"outcome":"accepted","state":{"holder":"ben","renewals":0,"waiting":[]}}
```

`event(action, args=None, *, accepted=True, state=None)` normalizes values as the runners do: `None`
is nothing, `set` becomes an array, enum members become their names. `reset()` starts a new run;
`close()` (or the `with` block) closes a file the recorder opened.

## Recording from JavaScript

```js
import { Recorder } from "zin-verify";

const log = new Recorder("events.jsonl");    // or a function (line) => ..., or nothing
log.event("borrow", { m: "ana" }, { state: { holder: "ana" } });
log.event("renew", [], { accepted: false });
log.reset();
```

Without a target the lines are kept in `log.lines` and `log.text()` returns them as one log.
`zin-verify/recorder` exports the same `Recorder` with no Node.js dependency, so code running in a
browser can keep its own log and hand it over at the end:

```tsx
import { Recorder } from "zin-verify/recorder";

const session = new Recorder();          // or new Recorder((line) => send(line))
<Editor onEvent={(action) => session.event(action)} />
```

## Where to record

Record an event **where it takes effect**, not where it is requested:

- in the request handler after the transaction commits, not when the request arrives;
- in a click handler for a user's action, and in the resolved promise for a server's answer;
- under the same lock that makes the call atomic, if the system is concurrent, so the log order is the
  order of effect.

Record refusals too. A log with only accepted calls cannot catch an implementation that wrongly
refuses, and a refused call recorded as accepted is reported as a violation.

## Traces and concurrency

Conformance tests drive one call at a time. A trace is how you check a real concurrent run: the model
explores every interleaving, and `zin trace` checks that the interleaving that actually happened is one
of them. `examples/react-editor` clicks 150 random enabled buttons against a server that answers late
and fails one time in four, then runs `zin trace` on the page's own log. See
[checking a web interface](../browser.md#sessions-nobody-scripted).
