---
title: Types
parent: Language
nav_order: 1
---

# Types
{: .no_toc }

Every field and every parameter has a finite type. Finite is the point: the
checker enumerates every value, so a type is also a promise about how much
work exploration does.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Overview

| Type | Written as | Values |
| --- | --- | --- |
| Bounded integer | `an integer from 0 to 20` | `0` … `20` |
| Boolean | `a boolean` | `true`, `false` |
| Enumeration | `one of open, closed` | `open`, `closed` |
| Named type | `a worker`, `worker` | whatever `worker` was declared as |
| Map | `a map from worker to a boolean` | one value per key |
| Record | `a record with (status is ..., items is ...)` | one value per member |
| Optional | `a worker or nothing` | a worker, or `nothing` |
| Set | `a set of worker` | any selection of workers |
| List | `a list of at most 3 message` | 0 to 3 messages, in order |
| Enumeration with members | `one of idle, running with (attempt is ...)` | `idle`, or `running` with an attempt |

The scalar types (integers, booleans, enumerations) are the only ones allowed
as action parameters, quantifier ranges and map keys. Compound types may nest
inside each other freely.

## Integers

```zin
state
  balance is an integer from 0 to 20
  delta is an integer from -3 to 3
```

Both bounds are inclusive and may be negative. Intermediate arithmetic may
leave the range; an assignment may not:

```console
$ zin check tank.zin
model: tank.zin:7:3: add: balance would become 4, but it is an integer from 0 to 3; add a 'requires' that rules this out
counterexample:
  start: balance=0
  1. add  ->  balance=1
  2. add  ->  balance=2
  3. add  ->  balance=3
```

The fix is almost always a `requires` (`requires balance < 3`), which turns
the impossible step into a rejection the tests will check.

{: .tip }
Keep ranges small. Three values of a counter find the same off-by-one
defects as a hundred, and the plan for `0 to 100` has fifty times as many
steps as the plan for `0 to 3`. See [keeping ranges small](patterns.md#keep-ranges-small).

In plans an integer is a JSON number: `{"kind": "integer", "min": 0, "max": 20}`.

## Booleans

```zin
state
  locked is a boolean
initially
  locked is false
action lock
  requires not locked
  locked becomes true
```

A boolean is a condition on its own: `requires locked`, `always not locked or
...`. Booleans are a distinct type from integers: `locked is 0` is a type
error. In plans, `true` and `false`.

## Enumerations

```zin
state
  status is one of draft, pending, sent
```

An enumeration is a closed list of words. `one of open, closed or frozen`
(with a final `or`) also reads naturally and means the same.

- Enum values compare with `is` and `is not` only; `<` and friends take
  integers.
- One value may belong to several enumerations (`door` and `window` may both
  be `open`). The surrounding comparison or assignment says which one is
  meant.
- Quote a value that would otherwise read as a keyword: `"on"`, `"nothing"`,
  `"and"`. See [quoting](index.md#quoting-enum-values).

Type errors are reported before anything runs:

```console
$ zin check mixed.zin
spec: mixed.zin:8:8: cannot compare a number with an enum value
  = see: zin explain type.mismatch
```

In plans an enum value is a string, and an enum parameter arrives in your
adapter as its name in the specification: `"open"`.

## Named types

```zin
system "Thermostat"

types
  mode is one of off, heat, cool
  level is an integer from 0 to 3

state
  target is level
  mode is mode
  locked is a boolean

initially
  target is 1
  mode is off
  locked is false

action set(t is a level)
  requires not locked
  target becomes t

action switch(m is a mode)
  requires not locked
  requires mode is not m
  mode becomes m

action lock
  requires not locked
  locked becomes true
```

```console
$ zin check thermostat.zin
ok Thermostat: 24 states, 84 transitions, 0 properties, 0 scenarios
```

A `types` block names a type once so fields, parameters, map keys and
quantifiers can share it. Refer to it as `a level` or plain `level`; the
article is optional.

Named types are required in three places:

- **Map keys**: `a map from worker to ...` needs `worker` declared.
- **Set elements**: `a set of worker`.
- **Quantifiers over numbers**: `every n in level satisfies ...`. A
  quantifier ranges over a named type or a set or list field, never over an
  inline `an integer from ...`.

```console
$ zin check quantnum.zin
spec: quantnum.zin:6:21: expected 'satisfies', found 'integer'
```

## Interchangeable values

```zin
types
  interchangeable worker is one of ann, ben, cid, dot
```

Declare an enumeration `interchangeable` when nothing in the specification
treats one of its values differently from another: identical workers,
replicas, connections in a pool. `zin check` then explores one state per
arrangement *up to renaming*: four workers waiting on a lock in
[`examples/mutex.zin`](https://github.com/enekos/zin/blob/main/examples/mutex.zin)
take 9 states instead of 48.

```console
$ zin check examples/mutex.zin
ok Lock shared by interchangeable workers: 9 states up to symmetry, 30 transitions, 4 properties, 1 scenario
$ zin check --symmetry off examples/mutex.zin
ok Lock shared by interchangeable workers: 48 states, 144 transitions, 4 properties, 1 scenario
```

Counterexamples are still printed as ordinary runs with real names.

The checker holds you to the promise:

- **Actions, properties and `initially` may not name an interchangeable
  value.** Reach the values through parameters (`enter(w is a worker)`),
  quantifiers and `of every`. Scenarios may name them, since a scenario is
  one concrete run.

  ```console
  $ zin check sym-name.zin
  spec: sym-name.zin:12:18: ann is an interchangeable worker, so the specification cannot single it out; refer to it through a parameter or a quantifier
  ```

- **A map literal keyed by an interchangeable type gives every key the same
  value.** `load becomes {ann: 0, ben: 0, cid: 0}` is fine;
  `{ann: 0, ben: 0, cid: 2}` names `cid` as surely as `load of cid` does.

- **Another enumeration holds all of the values or none.** `holder is one of
  ann, ben, cid, dot, nobody` is fine; a subset is not:

  ```console
  $ zin check sym-part.zin
  spec: sym-part.zin:5:3: this enumeration shares values with the interchangeable type worker; list all of its values (ann, ben) or none of them
  ```

- **At most 8 interchangeable values in total** (8! = 40,320 renamings).

Where the reduction would be unsound, the checker uses the full state space
and says so: liveness under fairness (fairness is per call, and renaming
changes which call is which), and quantifiers over interchangeable values
that could fault for one value but not another. Plans, `zin trace` and
diagrams always use the full state space, because tests drive real values.

## Maps

```zin
system "Votes"
types
  voter is one of ann, ben, cid
  choice is one of none, yes, no
state
  vote is a map from voter to choice
  open is a boolean
initially
  vote of every voter is none
  open is true
action cast(v is a voter, c is a choice)
  requires open
  requires vote of v is none
  requires c is not none
  vote of v becomes c
action close
  requires open
  open becomes false
action reset
  requires not open
  vote of every voter becomes none
  open becomes true
always not open or (number of v in voter where vote of v is yes) <= 3
sometimes every v in voter satisfies vote[v] is yes
```

```console
$ zin check votes.zin
ok Votes: 54 states, 108 transitions, 2 properties, 0 scenarios
```

- **Keys** are a named finite type: an enumeration, a boolean or a bounded
  integer declared under `types`.
- **Values** may be any type, including maps and records.
- **Every key always has a value.** There is no "missing key"; model absence
  with an optional value (`a map from worker to a task or nothing`).
- **Reading**: `vote of v` or `vote[v]`. Nested maps read with brackets:
  `paid_on[c][tue]`.
- **Writing**: `vote of v becomes yes` writes one entry; `vote of every voter
  becomes none` writes all of them.
- **Literal**: `{mon: false, tue: false}` gives every key a value. A literal
  that misses a key is an error:

  ```console
  $ zin check maplit.zin
  spec: maplit.zin:7:3: this map needs a value for every key, and tue has none
  ```

- **Initial values**: every key needs one. `of every` gives a default that
  single entries refine (see [initial values](state-and-actions.md#initial-values)).

  ```console
  $ zin check mapbad.zin
  mapbad.zin: spec: vote of ben has no initial value; set it in the initially block
  ```

- A key outside the key type is an error, caught statically where it can be:

  ```console
  $ zin check balancer.zin
  spec: balancer.zin:10:28: load is keyed by one of s1, s2, s3, but the key here is s1 or s2 or s3 or none
    = see: zin explain type.mismatch
  ```

In plans a map is an object keyed by the key's text; the field type lists the
keys as strings:

```json
{"name": "vote", "type": {"kind": "map", "keys": ["ann", "ben", "cid"],
  "value": {"kind": "enum", "values": ["none", "yes", "no"]}}}
```

```json
{"open": true, "vote": {"ann": "none", "ben": "none", "cid": "none"}}
```

## Records

```zin
types
  order is a record with (status is one of draft, paid, shipped, items is an integer from 0 to 2)

state
  orders is a map from customer to order
  last is a record with (who is one of ann, ben, nobody, what is order)
```

A record groups values that belong together, so they cannot drift apart the
way two parallel maps can. Members are separated by commas, like action
parameters, and may have any type, including other records.

- **Reading** goes right to left: `status of orders of c` is the entry `c` of
  `orders`, then its member `status`. `name of x` is a member when `x` is a
  record and a map entry when `name` is a map; the types decide.
- **Literal**: `{status: paid, items: 2}`; members in any order. The literal
  takes its type from the other side of `is` or `becomes`.
- **Comparison**: `is` and `is not` compare whole records member by member:
  `orders of c is {status: shipped, items: 2}`.
- **Assignment**: any path. `items of orders of c becomes 1`, `orders of c
  becomes {status: shipped, items: 2}`, `status of orders of every customer
  becomes draft`.
- **One write per slot per action.** Assigning a whole record and one of its
  members in the same action is an error:

  ```console
  $ zin check rec-twice.zin
  spec: rec-twice.zin:8:3: this value is already assigned in the same block
  ```

The full example is
[`examples/orders.zin`](https://github.com/enekos/zin/blob/main/examples/orders.zin).
In plans a record is an object keyed by member name:

```json
{"orders": {"ann": {"items": 2, "status": "draft"}, "ben": {"items": 0, "status": "draft"}}}
```

## Optional values

```zin
types
  interchangeable worker is one of ann, ben, cid
  ticks is an integer from 1 to 2

state
  lease is a record with (holder is a worker, left is ticks) or nothing
  last is a worker or nothing

initially
  lease is nothing
  last is nothing

action acquire(w is a worker)
  requires lease is nothing
  lease becomes {holder: w, left: 2}
  last becomes w
```

`<type> or nothing` holds a value of the type or no value. Use it instead of
a made-up sentinel: no fake `nobody` worker, no fake `0` for "not asked yet".

- **Comparison**: `lease is nothing`, `last is not nothing`, `last is w`
  (false when `last` is nothing).
- **Reading the value** happens implicitly wherever a plain value is needed:
  arithmetic, ordering, a map key, a member (`left of lease`). Reading it when
  it is nothing is a model fault:

  ```console
  $ zin check question.zin
  model: question.zin:12:3: answer: asked is nothing
  counterexample:
    start: asked=nothing, total=0
  ```

  Guard the read (`lease is not nothing and left of lease is 1`) or give a
  default (`left of lease or else 0`).
- **Assignment** takes a value or `nothing`; an `if` may mix them:
  `lease becomes if left of lease is 1 then nothing else {...}`.
- **An optional record is assigned whole.** Its members cannot be assigned
  one by one:

  ```console
  $ zin check opt-member.zin
  spec: opt-member.zin:13:3: only a state field, or an entry or member of one, can be assigned
  ```

- **Types stay strict.** Assigning an optional into a plain field is a type
  error, even when you know it is set:

  ```console
  $ zin check opt-type.zin
  spec: opt-type.zin:9:3: total is an integer from 0 to 3, but 'total becomes asked' assigns a number or nothing
    = see: zin explain type.mismatch
  ```

In plans, nothing is `null`; a present value is encoded as the value itself:

```json
{"name": "last", "type": {"kind": "optional", "value": {"kind": "enum", "values": ["ann", "ben", "cid"]}}}
```

```json
{"last": null, "lease": null}
{"last": "cid", "lease": {"holder": "cid", "left": 2}}
```

## Sets

```zin
system "Rooms"

types
  interchangeable worker is one of ann, ben, cid

state
  inside is a set of worker
  visited is a set of worker
  open is a boolean

initially
  inside is []
  visited is []
  open is true

always
  every w in inside satisfies visited has w
  size of inside <= 2
never
  not open and inside is not []

action enter(w is a worker)
  requires open and not inside has w and size of inside < 2
  inside becomes inside with w
  visited becomes visited with w

action leave(w is a worker)
  requires inside has w
  inside becomes inside without w

action close
  requires open and inside is []
  open becomes false
```

A set holds any selection of a named type's values. Use it where you would
otherwise write `a map from worker to a boolean`: it says the same thing
directly.

| Expression | Meaning |
| --- | --- |
| `[ann, ben]`, `[]` | a set literal; it takes its type from the other side of `is` or `becomes` |
| `inside has w` | membership (a comparison) |
| `inside with w` | the set plus `w`; with another set, the union |
| `inside without w` | the set minus `w`; with another set, the difference |
| `size of inside` | the number of members |
| `every w in inside satisfies ...` | quantify over the members |
| `inside is [ann]` | whole-set equality; order in the literal does not matter |

`with` and `without` bind more loosely than arithmetic and more tightly than
comparisons, so `inside with w is [ann]` compares the union.

The element type must be named; `a set of an integer from 0 to 2` does not
parse, but `a set of slot` with `slot is an integer from 0 to 2` does.

In plans a set is an array of its members in the order the element type
declares them; runners and `zin trace` accept any order from your `observe`.
Observe a native set where your language has one (a Python `set`, a
JavaScript `Set`, a Java `Set`, Go's `zin.Set`).

```json
{"name": "inside", "type": {"kind": "set", "values": ["ann", "ben", "cid"]}}
```

```json
{"inside": ["cid"], "open": true, "visited": ["cid"]}
```

The full example is
[`examples/rooms.zin`](https://github.com/enekos/zin/blob/main/examples/rooms.zin).

## Lists

```zin
system "Channel"

types
  message is one of ping, pong

state
  queue is a list of at most 2 message
  received is a list of at most 3 message
  sent is an integer from 0 to 3

initially
  queue is []
  received is []
  sent is 0

always
  length of queue + length of received is sent

action send(m is a message)
  requires sent < 3 and length of queue < 2
  queue becomes queue with m
  sent becomes sent + 1

fair action deliver
  requires queue is not []
  received becomes received with first of queue
  queue becomes rest of queue
```

A list holds up to its capacity of items, in order, possibly repeated. Use it
for queues, logs and recency orders.

| Expression | Meaning |
| --- | --- |
| `[ping, pong]`, `[]` | a list literal; it takes its type from the other side of `is` or `becomes` |
| `queue with m` | `m` appended at the end; appending to a full list is a model fault |
| `queue without m` | every item equal to `m` removed, the rest kept in order |
| `queue has m` | whether some item equals `m` |
| `length of queue` | the number of items |
| `first of queue`, `last of queue` | an item; a fault on an empty list |
| `rest of queue` | all but the first item; a fault on an empty list |
| `first of queue or else ping` | the first item, or `ping` when empty |
| `every m in queue satisfies ...` | quantify over the items, in order |
| `queue is [pong]` | item-by-item equality |

Appending past the capacity is caught by the checker:

```console
$ zin check list-full.zin
model: list-full.zin:9:3: send(ping): queue is full
counterexample:
  start: queue=[]
  1. send(ping)  ->  queue=[ping]
  2. send(ping)  ->  queue=[ping, ping]
```

`queue without m with m` moves `m` to the end, which is how
[`examples/recency.zin`](https://github.com/enekos/zin/blob/main/examples/recency.zin)
keeps a least-recently-used order. In plans a list is an array:

```json
{"name": "order", "type": {"kind": "list", "capacity": 2, "element": {"kind": "enum", "values": ["a", "b", "c"]}}}
```

## Enumerations with members

```zin
system "Retried job"

types
  attempt is an integer from 1 to 3

state
  job is one of queued, running with (attempt is attempt), succeeded, failed with (reason is one of crashed, timed_out)
  tries is an integer from 0 to 3

initially
  job is queued
  tries is 0

always
  job is running implies attempt of job is tries

fair action start
  requires job is queued
  job becomes running with {attempt: tries + 1}
  tries becomes tries + 1

action succeed
  requires job is running
  job becomes succeeded

action crash
  requires job is running
  job becomes if attempt of job is 3 then failed with {reason: crashed} else queued
```

A case of an enumeration may carry members that exist only while it holds:
the attempt number while running, the reason once failed. With a plain status
enum and side fields, `attempt` would keep a stale value in every other state;
here it cannot.

- **Writing a value**: a case without members is its name (`queued`); a case
  with members is `running with {attempt: 2}`. Naming it bare is an error:

  ```console
  $ zin check var-bare.zin
  spec: var-bare.zin:8:3: running carries members; write 'running with {attempt: ...}'
    = see: zin explain type.mismatch
  ```

- **Testing the case**: `job is running` is true whatever the attempt; `job
  is running with {attempt: 3}` also compares the members.
- **Reading a member**: `attempt of job`. When the current case has no such
  member it is a model fault:

  ```console
  $ zin check var-fault.zin
  model: var-fault.zin:12:3: peek: job is not running, so it has no attempt
  counterexample:
    start: job=idle, n=0
  ```

  Guard it (`job is running and attempt of job is 3`) or default it
  (`attempt of job or else 0`).
- **Members are assigned with the whole value**, never one by one: write
  `job becomes running with {attempt: attempt of job + 1}`.

In plans a case without members is a string, and a case with members is an
object with one key, the case:

```json
{"job": "queued", "tries": 1}
{"job": {"running": {"attempt": 2}}, "tries": 2}
```

The full example is
[`examples/retry.zin`](https://github.com/enekos/zin/blob/main/examples/retry.zin).

## What a type costs

The checker stores a state as a vector of small integers, one per *slot*:

| Type | Slots |
| --- | --- |
| integer, boolean, enumeration | 1 |
| record | the sum of its members |
| map | one value's slots per key |
| set | one per element value |
| optional | 1 (present or not) plus the value's |
| list | 1 (length) plus capacity × the item's |
| enumeration with members | 1 (the case) plus every case's members |

A specification may have at most 4,096 slots. The number of *reachable*
states, not slots, is what exploration pays for, and that is bounded by your
`requires` lines as much as by your types.

## Encoding summary

How each value appears in a plan (`zin plan`), in `zin trace` events, and in
what your adapter's `observe` returns:

| Type | JSON |
| --- | --- |
| integer | number |
| boolean | `true` / `false` |
| enumeration | string |
| map | object keyed by the key's text |
| record | object keyed by member name |
| optional | the value, or `null` |
| set | array in the element type's order (any order accepted from `observe`) |
| list | array in order |
| case without members | string |
| case with members | `{"case": {members}}` |

The [plan format](../plan-format.md) is the full contract.
