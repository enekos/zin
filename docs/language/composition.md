---
title: Composition and includes
parent: Language
nav_order: 6
---

# Composition and includes
{: .no_toc }

Larger systems are built from components: several `system` blocks that share
no state and synchronize on actions with the same name. Components can live in
separate files and be tested on their own.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Several systems in one file

```zin
system "Client"

state
  waiting is a boolean
  asked is an integer from 0 to 3

initially
  waiting is false
  asked is 0

action request(n is an integer from 1 to 3)
  requires not waiting
  waiting becomes true
  asked becomes n

action reply(n is an integer from 1 to 3)
  requires waiting and asked is n
  waiting becomes false

system "Server"

state
  job is an integer from 0 to 3
  done is a boolean

initially
  job is 0
  done is false

action request(n is an integer from 1 to 3)
  requires job is 0
  job becomes n

fair action work
  requires job is not 0 and not done
  done becomes true

fair action reply(n is an integer from 1 to 3)
  requires done and job is n
  job becomes 0
  done becomes false

system "Request and reply"

always
  waiting implies job is asked
never
  done and not waiting
whenever waiting eventually not waiting
always possible not waiting

scenario "A request is answered"
  request(2)
  reject reply(2)
  work
  reply(2)
  expect not waiting and job is 0
```

```console
$ zin check request-reply.zin
ok Request and reply: 10 states, 18 transitions, 4 properties, 1 scenario
```

This is [`examples/request-reply.zin`](https://github.com/enekos/zin/blob/main/examples/request-reply.zin).
The rules:

- **Components share no state.** Field names must differ across blocks, and a
  block's actions and `initially` read and write only its own fields.
- **An action declared in several blocks is one step they all take at
  once.** `request(n)` is enabled only when the client is not waiting *and*
  the server is idle, and it applies both blocks' assignments. This is the
  synchronized product of CSP-style processes.
- **Shared parameters carry data.** Parameters with the same name are the same
  value and must have the same type: `n` passes the request number from
  client to server. The merged action takes the first block's parameters,
  then any new ones, in order.
- **Every other action interleaves.** `work` belongs to the server alone and
  changes only the server's state.
- **Fairness**: a shared action is as fair as the fairest block declares it.
- **Properties, `terminal when` and scenarios describe the whole** and may
  name any block's fields. Put them in the block they concern, or in a block
  without state whose name then names the composition. Without such a block
  the composition is named after its parts (`Client + Server`). Several
  `terminal when` lines must all hold.
- **Types with the same name** must be declared the same way in every block.
- **The largest `explore at most` wins.**

## Composition errors

Each rule has its own diagnostic code (see [diagnostics](diagnostics.md)):

```console
$ zin check c1.zin
spec: c1.zin:15:3: action poke of B assigns x, which belongs to A; a component reaches another only through shared actions
  = see: zin explain component.foreign_field
```

```console
$ zin check c2.zin
spec: c2.zin:9:3: x is already a state field of A; components share no state, so rename one and pass values through a shared action (first declared at c2.zin:3:3)
  = see: zin explain component.field_conflict
```

```console
$ zin check c3.zin
spec: c3.zin:14:13: B gives parameter v of the shared action send the type an integer from 0 to 1, but another component gives it a boolean (first declared at c3.zin:6:13)
  = see: zin explain component.parameter_conflict
```

```console
$ zin check tc.zin
spec: tc.zin:11:3: B declares type color differently; components that share a type must declare it the same way (first declared at tc.zin:3:3)
  = see: zin explain component.type_conflict
```

## Includes

```zin
include "parts/client.zin"
include "parts/server.zin"

system "Messaging"
always pending is at most 1
sometimes seen is 2
```

```console
$ zin check entry.zin
ok Messaging: 5 states, 4 transitions, 2 properties, 0 scenarios
```

A file may begin with `include` directives, then continue with complete
`system` blocks. An entry file may hold only includes. Inclusion is textual,
with these deliberate choices:

- **Included text comes first.** Each file's includes, in directive order,
  precede its own blocks. The combined text is checked exactly as if written
  in one file, so a composition behaves the same split or whole.
- **Each file is read once**, however many files include it.
- **One flat namespace.** No imports, qualified names or private declarations.
  Included files share types, fields and actions by name, under the
  composition rules above.
- **Paths are relative** to the including file, use `/` separators, and stay
  within the entry file's directory:

  ```console
  $ zin check esc.zin
  spec: esc.zin:1:9: include paths must stay within the entry file's directory
  ```

- **Cycles are errors**, reported at the include that closes them:

  ```console
  $ zin check loop.zin
  spec: loop.zin:1:9: cyclic include of loop.zin
  ```

- **A missing file suggests the closest `.zin` file** next to it:

  ```console
  $ zin check entry.zin
  usage: entry.zin:1:9: cannot read parts/clinet.zin: No such file or directory (os error 2)
    hint: did you mean `"parts/client.zin"`?
      1 | include "parts/client.zin"
        |         ^^^^^^^^^^^^^^^^^^
  ```

- **Errors point into the file that holds the text**, not the entry file:

  ```console
  $ zin check entry.zin
  spec: parts/client.zin:5:3: unknown name pendng; declare it or use a quoted enum value
    hint: did you mean `pending`?
      5 |   pending is 0
        |   ^^^^^^^
    = see: zin explain name.unknown
  ```

- **Limits**: at most 1,024 files, nested 64 levels deep, 4 MiB in total.

Standard input (`zin check -`) holds one self-contained specification;
`include` there is an error.

### Includes and committed plans

A plan records every included file's hash under `sources`, and its
`fingerprint` covers all of them, so editing any included file makes a
committed plan stale. Bindings detect it and fail with the command to
regenerate the plan, never with a stale pass.

```json
{"fingerprint": "97369998594fbcec",
 "sources": {"parts/client.zin": "b169f9126fe071b9", "parts/server.zin": "03c79ca3283edee0"}}
```

In Rust, `zin::parse_file(path)` loads a file with its includes and
`assert_conforms_file` tests one; `zin::parse(text)` treats `include` as an
error.

## Testing one component

The whole composition is checked, but you can test a single block's
implementation against it:

```console
$ zin plan request-reply.zin --component Client --out client.plan.json
ok Client: 7 states, 15 transitions, 0 properties, 1 scenario
plan: 3 tests, 26 accepted steps, 27 rejected steps (cover: transitions) -> client.plan.json
```

Zin checks the whole composition first, then builds a plan for the named block
alone:

- The adapter implements only that block's actions (`request`, `reply`) and
  observes only its fields (`waiting`, `asked`).
- The other blocks become a **controllable environment**: it may offer a
  shared action whenever the component's own `requires` allow it. So the plan
  exercises local behavior that a particular server might never request:
  for the client, the server-only `work` disappears and a `reply` can follow a
  `request` at once.
- Whole-system scenarios seed the plan. Each keeps the component's calls and
  its own refusals; a call only the environment takes or refuses never
  reaches the adapter.
- Properties and `terminal when` describe the whole and are checked there,
  not on the component.
- The plan's `component` field names the block (`null` for the whole system).

Every binding takes the component as a planning option (`component=` in
Python, `{ component }` in JavaScript, `zin.WithComponent` in Go,
`Options.component` in JUnit). An unknown name lists the choices:

```console
$ zin plan request-reply.zin --component Nope
unknown component 'Nope'; available: Client, Server, Request and reply
```

## When to split

- Split when two parts of a system have their own state and talk through a
  narrow interface: client and server, producer and consumer, UI and backend.
  The shared actions are that interface.
- Keep one block when the parts read each other's state directly. Forcing
  them apart means copying state across through actions, which is more model
  than the system has.
- Use `include` to keep files short and to let teams own their component's
  file; the composition rules keep them honest.
