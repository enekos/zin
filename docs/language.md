# The Zin language

A Zin file describes a finite-state system in six kinds of declarations: identity, state, initial values, promises, actions, and scenarios. Blank lines and `#` comments are free-form. Blocks use spaces for indentation; two spaces are conventional.

## A complete skeleton

```zin
system "Reservations"
explore at most 5000 states

state
  seats is an integer from 0 to 2
  phase is one of booking, closed
  audited is a boolean

initially
  seats is 2
  phase is booking
  audited is false

always
  seats is at least 0
  phase is closed implies seats is 0

action reserve
  requires phase is booking
  requires seats is greater than 0
  seats becomes seats - 1

scenario "The last reservation"
  reserve
  reserve
  expect seats is 0
```

`system` must come first. `state` and `initially` are each required and may appear once. `explore` is optional and defaults to 10,000 states. Multiple `always` blocks are allowed. Action and scenario names may be identifiers or quoted strings; field and parameter names are identifiers. Expression keywords (`true`, `false`, `not`, `and`, `or`, `implies`, `is`, `equals`, `plus`, `minus`, `times`, and `divided`) are reserved and cannot be field or parameter names.

## State and initial values

Three finite types are available:

```zin
count is an integer from -10 to 10
mode is one of idle, active, stopped
enabled is a boolean
```

Integers are signed 64-bit values. Literals cover the complete range from `-9223372036854775808` through `9223372036854775807`; arithmetic overflow is always an error.

Enum values may be quoted when they contain spaces, punctuation, or reserved words. Quote values such as `"true"`, `"false"`, and `"not"` when they could be confused with expression syntax. Every state field must have exactly one initial assignment. Initial right-hand sides must not refer to state or parameters. Initial values and every later assignment are checked against the declared domain.

Domains are semantic constraints, not promises. An enabled action that tries to leave a domain is a model error. Use `requires` to make the action unavailable instead.

## Promises

An `always` block contains boolean expressions that must hold in the initial state and every reachable state:

```zin
always
  balance is at least 0
  status is closed implies balance is 0
```

A promise can also be written on one line:

```zin
always balance is at least 0
```

On failure, Zin returns the promise, complete state, and a shortest action trace from the initial state.

## Actions

An action has optional finite-domain parameters, any number of preconditions, and deterministic assignments:

```zin
action transfer(amount is an integer from 1 to 10)
  requires phase is open
  requires amount is at most balance
  balance becomes balance - amount
  sent becomes sent + amount
```

An action is enabled only when all its `requires` conditions are true. The checker tries every parameter combination in every reachable state. A call with no preconditions is always enabled.

All right-hand sides read the **pre-action state**. Assignments then commit simultaneously, making swaps and other coupled updates unambiguous. Fields not assigned by the action remain unchanged.

Every expression is statically typed before exploration, including expressions inside unreachable actions. Conditions must be boolean; arithmetic and ordering require integers; assignments must match the target field. Enum assignments and comparisons also check their declared value domains, catching misspellings even when written as quoted literals.

## Expressions

English operators and symbolic operators are interchangeable:

| Meaning | English | Symbol |
|---|---|---|
| Equal | `is`, `is equal to`, `equals` | `=`, `==` |
| Not equal | `is not`, `is different from` | `!=` |
| Less / at most | `is less than`, `is at most` | `<`, `<=` |
| Greater / at least | `is greater than`, `is at least` | `>`, `>=` |
| Add / subtract | `plus`, `minus` | `+`, `-` |
| Multiply / divide | `times`, `divided by` | `*`, `/` |
| Logic | `not`, `and`, `or`, `implies` | — |

Parentheses override precedence. From tightest to loosest: numeric negation; multiplication; addition; comparisons; `not`; `and`; `or`; `implies`. Thus `not status is open` means `not (status is open)`. Implication is right-associative. `and`, `or`, and `implies` short-circuit at runtime, but both sides are always statically type-checked. Arithmetic is checked for overflow and division by zero.

Bare names resolve to state fields or parameters first, then to declared enum values. Unknown names are rejected so a misspelled field or enum cannot quietly change a proof.

## Scenarios

Scenarios are executable review journeys from the common initial state:

```zin
scenario "Reserve the last seat"
  reserve
  expect seats is 1
  reserve
  expect seats is 0
```

Call actions positionally or by name:

```zin
transfer(5)
transfer with amount = 5
move with target = away and speed = 2
```

`expect` checks a condition at the current point. Calling a disabled action, supplying wrong arguments, breaking a promise, or failing an expectation stops verification with the journey and state in the error.

Scenarios supplement exhaustive exploration; they do not replace it. They are useful as executable examples, review narratives, and blueprints for implementation conformance tests.

## Mermaid output

`zin diagram FILE` emits a `stateDiagram-v2` chart of reachable states and enabled transitions. Large charts are visually capped at 40 states and 120 transitions with an explicit Mermaid comment; verification itself is never capped by those display limits.

`zin diagram FILE --scenario NAME` emits a `sequenceDiagram` with every call and resulting state. `zin report` embeds both in a Markdown document containing domains, promises, action contracts, coverage notes, journeys, and verification totals.

## Formal envelope

Zin currently proves finite **safety** properties over deterministic models. State and action domains must be finite. The default limit is 10,000 reachable states; argument-product and transition guards prevent accidental resource exhaustion. Any reached guard is an explicit error.

The model does not execute implementation code and makes no assumptions about Rust or any other language. Concurrency, time, external services, and retries must be modeled explicitly as state and actions. Liveness and unbounded verification are intentionally outside the current proof envelope.

For a practical specification-to-implementation workflow, see [integrating with a real app](integration.md).
