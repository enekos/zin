# The Zin language

A specification is a UTF-8 text file. Structure comes from indentation: a
heading line at the left margin, its body indented beneath it. `#` starts a
comment. Blank lines are ignored.

```zin
system "Name"                      # required, first
explore at most 100000 states      # optional; default 1,000,000

types                              # optional named types
  worker is one of alice, bob

state                              # the variables
  ...
initially                          # one value for every variable
  ...

always / never / sometimes / eventually     # properties
whenever <p> eventually <q>
always possible <condition>
terminal when <condition>

action name(parameters)            # how the state can change
  ...
scenario "Name"                    # worked examples, checked and turned into tests
  ...
```

## Types

| Type | Written as |
| --- | --- |
| Bounded integer | `an integer from 0 to 20` |
| Boolean | `a boolean` |
| Enumeration | `one of open, closed` or `one of open, closed or frozen` |
| Named type | `a worker` / `worker` (declared under `types`) |
| Map | `a map from worker to a boolean` |
| Record | `a record with (status is one of draft, paid, items is an integer from 0 to 2)` |
| Optional | `a worker or nothing`, `an integer from 1 to 3 or nothing` |
| Set | `a set of worker` |
| List | `a list of at most 3 message` |
| Enumeration with members | `one of queued, running with (attempt is an integer from 1 to 3), succeeded` |

Map keys must be a named finite type (an enum, a boolean, or a bounded
integer); map values may be any type, including maps and records. Every key
always has a value.

Enum values are plain words. Quote one (`"on"`) when it would otherwise read as
a keyword or as another name. An enum value may belong to several enums; where
that makes an expression ambiguous the checker says so.

### Interchangeable values

```zin
types
  interchangeable worker is one of ann, ben, cid, dot

state
  at is a map from worker to phase
  holder is one of ann, ben, cid, dot, nobody
```

Declare an enumeration `interchangeable` when nothing in the specification
treats one of its values differently from another: four identical workers,
three replicas, a pool of connections. `zin check` then explores one state
for each arrangement up to renaming the values (`examples/mutex.zin`: 9
states instead of 48), and a counterexample is still reported as an ordinary
run with the real names.

The checker holds you to it:

- Actions, properties and initial values may not name an interchangeable
  value. Reach them through parameters (`enter(w is a worker)`), quantifiers
  and `of every`. Scenarios may name them, since they describe one run.
- Another enumeration holds all of a type's interchangeable values or none:
  `one of ann, ben, cid, dot, nobody` is fine, `one of ann, nobody` is not.
- At most 8 interchangeable values in total (8! relabelings).

Conformance plans, `zin trace` and diagrams always use the full state space,
because tests drive real values. Liveness under fairness is also checked on
the full state space: fairness is per call, and renaming changes which call
is which. So is a model where a `some` or `every` over interchangeable values
can fault (divide by zero, overflow, read a missing map key) for one value and
stop early on another: the order the quantifier visits values in then decides
whether the fault happens, and a representative could hide it. Zin judges this
from the declared ranges, so `load of w + 1` over a bounded map keeps the
reduction. `zin check` notes why it explored the full state space. `zin check
--symmetry off` turns the reduction off.

### Records and nested maps

```zin
types
  order is a record with (status is one of draft, paid, shipped, items is an integer from 0 to 2)

state
  orders is a map from customer to order
  paid_on is a map from customer to a map from day to a boolean

initially
  orders of every customer is {status: draft, items: 0}
  paid_on of every customer is {mon: false, tue: false}
```

A record groups values that belong together, so they cannot drift apart the
way two parallel maps can. Members are separated by commas, as action
parameters are, and may have any type.

- **Reading.** `status of orders of c` reads right to left: the entry `c` of
  `orders`, then its member `status`. `name of x` is a member when `x` is a
  record and a map entry when `name` is a map; the types decide. Index a map
  that is itself a map value with brackets: `paid_on[c][tue]`.
- **Literals.** `{status: paid, items: 2}` is a record; members may come in
  any order. The same braces write a whole map with one value per key:
  `{mon: false, tue: false}`. A literal takes its type from the other side of
  `is` or `becomes`.
- **Comparison.** `is` and `is not` compare whole records and maps, member by
  member; the other operators take integers.
- **Assignment.** Any path can be assigned: `items of orders of c becomes 1`,
  `orders of c becomes {status: shipped, items: 2}`,
  `status of orders of every customer becomes draft`. A path may use `of every`
  once, and every other key on it must then be a constant.
- **Limits.** Action parameters, quantifiers and map keys stay integers,
  booleans and enums.

Every slot of a record is part of the state: an order with two members takes
two slots, a map from two customers to orders takes four. Plans for a
specification with records or nested maps are [`plan/2`](plan-format.md).

### Optional values

```zin
state
  lease is a record with (holder is a worker, left is ticks) or nothing
  last is a worker or nothing

action acquire(w is a worker)
  requires lease is nothing
  lease becomes {holder: w, left: 2}
  last becomes w
```

`<type> or nothing` holds a value of the type or no value, so an absent
holder needs no made-up worker and an unasked question no made-up number 0.
`nothing` is the absent value.

- **Comparison.** `lease is nothing`, `last is not nothing`, and `last is w`,
  which is false when `last` is nothing.
- **Assignment.** Assign a value or `nothing`; `if` may mix the two.
- **Reading the value.** Where an integer, boolean or enum is needed
  (arithmetic, ordering, a condition, a map key) and wherever a member is read
  (`left of lease`), the value is used, and reading it when it is nothing is a
  model fault. Guard the read: `lease is not nothing and left of lease is 1`.
- An optional record is assigned whole; its members cannot be assigned one by
  one.

### Sets

```zin
state
  inside is a set of worker

initially
  inside is []

always
  every w in inside satisfies visited has w
  size of inside <= 2

action enter(w is a worker)
  requires not inside has w
  inside becomes inside with w
```

A set holds any selection of a named finite type's values, one slot per value.

| | |
| --- | --- |
| `[ann, ben]`, `[]` | a set; it takes its type from the other side of `is` or `becomes` |
| `inside has w` | membership, a comparison |
| `inside with w`, `inside without w` | the set plus or minus a value; with another set, union and difference |
| `size of inside` | the number of members |
| `every w in inside satisfies ...` | quantifiers range over a set field's members as well as over a type |

`with` and `without` bind more loosely than arithmetic and more tightly than
comparisons. Sets compare with `is`. In plans a set is an array of its members
in the order the element type declares them; runners and `zin trace` match it
in any order.

### Lists

```zin
state
  queue is a list of at most 2 message

initially
  queue is []

action send(m is a message)
  requires length of queue < 2
  queue becomes queue with m

action deliver
  requires queue is not []
  received becomes received with first of queue
  queue becomes rest of queue
```

A list holds up to its capacity of items in order; an item may be of any
type. It takes a length slot and the slots of every item it could hold.

| | |
| --- | --- |
| `[ping, pong]`, `[]` | a list; it takes its type from the other side of `is` or `becomes` |
| `queue with m` | `m` appended at the end; appending to a full list is a model fault |
| `queue without m` | every item equal to `m` removed, the rest kept in order |
| `queue has m` | whether some item equals `m` |
| `length of queue` | the number of items |
| `first of queue`, `last of queue` | an item; reading one from an empty list is a model fault |
| `rest of queue` | all but the first item; a fault when the list is empty |
| `every m in queue satisfies ...` | quantifiers range over a list field's items, in order |

Lists compare with `is`, item by item. `queue without m with m` moves `m` to
the end, which is how `examples/recency.zin` keeps a least recently used
order. In plans a list is an array.

### Enumeration values with members

```zin
state
  job is one of queued, running with (attempt is attempt), succeeded, failed with (reason is one of crashed, timed_out)

action start
  requires job is queued
  job becomes running with {attempt: tries + 1}

action crash
  requires job is running
  job becomes if attempt of job is 3 then failed with {reason: crashed} else queued
```

A value of an enumeration may carry members that exist only while it holds:
the attempt number while running, the reason once failed. A status enum with
side fields would let those fields hold stale values in every other state;
here they cannot.

- **Writing a value.** A case without members is its name (`queued`). A case
  with members is `running with {attempt: 2}`; naming it bare is an error.
- **Testing the case.** `job is running` is true whatever the attempt;
  `job is running with {attempt: 3}` also compares the members.
- **Reading a member.** `attempt of job` reads the member of the case that
  holds; when the case has no such member it is a model fault, so guard it:
  `job is running and attempt of job is 3`.
- Members are assigned with the whole value, never one by one.

In plans a case without members is a string and a case with members an
object with one key, the case: `{"running": {"attempt": 2}}`.

## State and initial values

```zin
state
  balance is an integer from 0 to 20
  at is a map from worker to phase

initially
  balance is 0
  at of every worker is idle
  at of alice is holding         # single entries refine 'of every'
```

Every variable, every key of every map and every member of every record must
receive an initial value.

## Expressions

From loosest to tightest binding:

| | |
| --- | --- |
| `a implies b` | right-associative |
| `a or b` | |
| `a and b` | |
| `not a` | |
| comparisons | `is`, `=`, `==`, `equals`, `is not`, `!=`, `is different from`, `<`, `<=`, `>`, `>=`, `is less than`, `is at most`, `is greater than` / `is more than`, `is at least` |
| `a + b`, `a - b` | also `plus`, `minus` |
| `a * b`, `a / b` | also `times`, `divided by`; division truncates toward zero |
| `-a` | |

Comparisons do not chain; parenthesize (`(a < b) is flag`).

Primary forms:

```zin
balance                       # a variable or parameter
5   true   closed             # literals
(balance + 1)
at of w        at[w]          # a map entry
if c then a else b            # both branches must have the same type
every w in worker satisfies at of w is done
some w in worker satisfies at of w is holding
number of w in worker where at of w is done
```

Quantifiers range over a named type (declare `small is an integer from 1 to 3`
under `types` to range over numbers) or over the members of a set field or the items of a list field and extend as far to the right as
possible; parenthesize them inside larger expressions.

Expressions are statically typed. Integers, booleans and each enum are
distinct: `balance is open` is an error, reported before anything runs, even
in actions that can never be enabled.

Evaluation is left to right and short-circuits: `and`, `or` and `implies`
skip their right side once the left decides the result, `if` evaluates one
branch, `some` stops at its first witness and `every` at its first
counterexample, in the type's declared order. A division by zero, an
arithmetic overflow or a key outside a map's key type in a part that is
evaluated is a model fault; in a part that is skipped it is not. So
`load of w is 0 or 2 / load of w is 1` is safe where `2 / load of w is 1`
alone faults.

[Language cases](language-cases.md) pairs each of these rules with a passing
specification and a one-line change that breaks it; the test suite checks
both versions.

## Actions

```zin
fair action deposit(amount is an integer from 1 to 5, w is a worker)
  requires status is open
  requires balance + amount is at most 20
  balance becomes balance + amount
  seen of w becomes balance
  at of every worker becomes idle
```

- An action is **enabled** for given arguments when every `requires` holds.
  Otherwise it is **rejected** and the state does not change.
- All assignments read the state **before** the action: `a becomes b` and
  `b becomes a` in one action swap the two.
- Assigning the same variable, map entry or record member twice in one action
  is an error, including through a whole record and one of its members.
  The one exception: `m of every <type>` sets a default that assignments to
  single entries refine.
- A variable not assigned keeps its value.
- An assignment that leaves a variable's declared range is a **model fault**,
  reported with the trace that reaches it. Add a `requires` to rule it out.
  The same applies to division by zero.
- Actions are deterministic. Model a choice made by the environment as a
  parameter, or as two actions.

Parameters range over finite types, so the checker tries every combination of
arguments in every state.

## Properties

Properties are written one per line under a heading, or on the heading line
(`never balance is 21`).

### Safety

```zin
always
  status is closed implies balance is 0
never
  balance is more than 20
```

Checked in every reachable state. A violation comes with the shortest sequence
of calls that reaches it.

### Reachability

```zin
sometimes
  status is closed
```

At least one reachable state satisfies the condition. Use this to make sure
the model is not vacuous: properties about closed accounts are worthless if no
account can be closed.

```zin
always possible status is open
```

From **every** reachable state some sequence of calls reaches the condition:
there is no point of no return.

### Liveness

```zin
eventually
  status is succeeded or status is failed

whenever status is running eventually status is not running
```

`eventually q` says every execution reaches `q`. `whenever p eventually q`
says that from every reachable state satisfying `p`, every execution reaches
`q` (possibly at once).

An **execution** is a path from a state that keeps taking steps for as long as
any call is enabled. Two consequences:

- A state where nothing is enabled ends the execution. If `q` has not happened
  by then, the property fails ("can stop before it happens").
- The system never idles while something is enabled. If your system may
  legitimately wait forever, model the waiting as an action.

Without fairness, a loop that avoids `q` is a violation even when an exit is
always available. Declare what the environment guarantees:

```zin
fair action finish
  requires not done
  done becomes true
```

`fair` is weak fairness, per call: if a call of a fair action stays enabled
forever, it is eventually taken. A violation then needs a loop in which, for
every fair call, the call is disabled somewhere on the loop or taken on it.
Weak fairness does not help when the call is only enabled on and off
(`examples/broken/stuck-job.zin`): a job that keeps crashing back to the queue
is never continuously ready to succeed.

```zin
strongly fair action succeed
  requires status is running
  status becomes succeeded
```

`strongly fair` is strong fairness, per call: if a call is enabled again and
again, forever, it is eventually taken. A violation then needs a loop that
takes every strongly fair call enabled anywhere on it. Use it for retries and
for a scheduler that keeps offering the same choice; it is a stronger promise
about the environment, so make sure the real one keeps it.

A liveness counterexample is a path followed by `then forever:` and a loop.

### Deadlock

```zin
terminal when status is closed
```

Without this line, states with no enabled call are allowed. With it, every
such state must satisfy the condition; any other is reported as a deadlock.

## Scenarios

```zin
scenario "Close and reopen an empty account"
  reject reopen                     # must be refused here
  close
  deposit with amount = 3           # named arguments
  deposit(3)                        # positional
  expect balance is 6 and status is open
```

Each scenario starts from the initial state. A call must be enabled, a
`reject` must be refused, an `expect` must hold. Scenarios document intended
behavior, pin it against regressions in the specification, and become the
first tests of every derived plan.

## Composition

Several `system` blocks in one file are checked as one system
(`examples/request-reply.zin`):

```zin
system "Client"
state
  waiting is a boolean
initially
  waiting is false
action request(n is an integer from 1 to 3)
  requires not waiting
  waiting becomes true

system "Server"
state
  job is an integer from 0 to 3
initially
  job is 0
action request(n is an integer from 1 to 3)
  requires job is 0
  job becomes n

system "Request and reply"         # no state: names the whole
always waiting implies job is not 0
```

- **Components share no state.** Field names must differ across blocks, and
  a block's actions and `initially` read and write only its own fields.
- **An action declared in several blocks is one step they all take at
  once.** It is enabled when every block's `requires` holds and applies every
  block's assignments. Parameters with the same name are the same value and
  must have the same type, which is how data passes between components; the
  merged action takes the first block's parameters, then any new ones, in
  order. It is as fair as the fairest block declares it.
- **Every other action interleaves**: it changes its own block's state only.
- **Properties, `terminal when` and scenarios describe the whole** and may
  name any block's fields. Put them in the block they concern, or in a block
  without state, whose name then names the composition. Several `terminal
  when` lines must all hold.
- Types with the same name must be declared the same way in every block that
  declares them. The largest `explore at most` wins.

Plans, traces and tests treat a composition as one system under test, unless
you test one component in isolation.

### Testing one component

```sh
zin plan examples/request-reply.zin --component Client --out client.plan.json
```

Zin checks the whole composition first, then builds a plan for the named
block alone. Its adapter implements only that block's actions and observes
only its fields. The other blocks become a controllable environment that may
offer a shared action whenever the component's own `requires` allow it, so the
plan can exercise local behavior that a particular implementation of the
other components never requests: for the client, the server-only `work`
disappears and a `reply` can follow a `request` at once.

Whole-system scenarios seed the plan. Each keeps the component's calls and its
own refusals; a call only the environment takes or refuses never reaches the
adapter. Properties and `terminal when` describe the whole and are checked
there, not on the component. The plan's `component` field names the block
(`null` for a whole system). In Rust, pass `verify_component(&spec, "Client")`
to `Plan::build`.

## Source files

A file may begin with `include` directives, then continue with complete
`system` blocks. An entry file may hold only includes.

```zin
include "parts/client.zin"
include "parts/server.zin"

system "Request and reply"
always pending is at most 1
```

Inclusion is textual, and these are deliberate choices:

- **Included text comes first.** Each file's included files, in directive
  order, precede its own blocks. The combined text is checked exactly as if it
  were written in one file, so a composition behaves the same split or whole.
- **Each file is read once.** A file included from several places, under any
  spelling of the same relative path, contributes its blocks once.
- **One flat namespace.** Included files share types, fields and actions by
  name, with the composition rules above. There are no imports, qualified
  names or private declarations. Two blocks that declare the same field, or a
  type or shared parameter differently, are an error that reports both
  declarations.
- **Paths are relative** to the including file, use `/` separators and stay
  within the entry file's directory. A cycle is an error at the include that
  closes it. At most 1,024 files nested 64 levels deep, 4 MiB in total.

`zin` commands read files this way; standard input (`-`) holds one
self-contained specification. In Rust, `parse_file(path)` loads a file with its
includes and `parse(text)` parses self-contained text, where `include` is an
error. A plan records each included file's fingerprint under `sources`, and
its `fingerprint` covers every file, so editing any of them makes a saved plan
stale.

## Diagnostics

Every error names the file, line and column it refers to:

```console
$ zin check entry.zin
spec: parts/client.zin:7:21: unknown name pendng; declare it or use a quoted enum value
```

The parser records where each name, condition, assignment and declaration
starts and ends, so a diagnostic points at the offending token, not merely its
line, even when the same name appears elsewhere on that line. An error that
concerns a whole statement covers the statement. A conflict between two
declarations also gives the first one.

`zin check --format json` adds a stable `code` (such as `name.unknown`,
`type.mismatch` or `component.field_conflict`), a `span` and a `related` span;
see [wire formats](plan-format.md#check1). In Rust, `Error::code`,
`Error::source_span` and `Error::related_span` return the same information.
Once an error leaves `parse`, `parse_file`, `Model::compile` or `verify`, its
`line` is a line of the file in `source_span`. AST nodes carry `Span`s in the
parser's coordinates; `Spec::source_span` resolves one to its file.

## Limits

Exploration stops with an error, never a partial answer, at the declared state
limit or at 20 million transitions. A specification may have at most 100,000
distinct calls (action × arguments) and 4,096 scalar slots. Nesting depth is
limited to 64.

## Notes

After a successful check the tool also notes actions that are never enabled in
any reachable state, since these are usually mistakes.
