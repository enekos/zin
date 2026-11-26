---
title: Expressions
parent: Language
nav_order: 3
---

# Expressions
{: .no_toc }

Expressions appear in `requires`, on the right of `becomes`, in properties
and in scenario `expect` lines. They are statically typed and evaluated left
to right with short-circuiting.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Operators and precedence

From loosest to tightest binding:

| Operator | Notes |
| --- | --- |
| `a implies b` | right-associative: `a implies b implies c` is `a implies (b implies c)` |
| `a or b` | |
| `a and b` | |
| `not a` | |
| comparisons | see below; they do not chain |
| `with`, `without` | sets and lists |
| `a + b`, `a - b` | also `plus`, `minus` |
| `a * b`, `a / b` | also `times`, `divided by`; division truncates toward zero |
| `-a` | |
| `a or else b` | a default for a value that may be absent |

So `not b and a is 0 or a > 0` reads as `((not b) and (a is 0)) or (a > 0)`,
and `-a + 2 * 3 / 2` as `(-a) + ((2 * 3) / 2)`. When in doubt, parenthesize;
parentheses never change the meaning of a correct expression.

### Comparisons

| Meaning | Spellings |
| --- | --- |
| equal | `is`, `=`, `==`, `equals` |
| not equal | `is not`, `!=`, `is different from` |
| less | `<`, `is less than` |
| at most | `<=`, `is at most` |
| greater | `>`, `is greater than`, `is more than` |
| at least | `>=`, `is at least` |
| membership | `has` (sets and lists) |

All spellings are interchangeable; pick the one that reads best. This
specification checks:

```zin
system "Synonyms"
state
  a is an integer from 0 to 6
initially
  a is 2
always
  a = 2 and a == 2 and a equals 2
  a != 3 and a is different from 3 and a is not 3
  a is less than 3 and a is at most 2 and a is greater than 1 and a is more than 1 and a is at least 2
  a times 3 divided by 2 is 3 and a plus 1 minus 1 is a
```

- `is` and `is not` compare any two values of the same type: integers,
  booleans, enum values, and whole records, maps, sets and lists.
- Ordering (`<`, `is at most`, ...) takes integers only.
- Comparisons do not chain. Write `0 <= a and a <= 3`, or parenthesize to
  compare a comparison: `(a < b) is flag`.

  ```console
  $ zin check chain.zin
  spec: chain.zin:6:15: unexpected '<='
  ```

### Arithmetic

Integers only. `/` truncates toward zero (`7 / 2` is `3`, `-7 / 2` is `-3`).
Intermediate results are not bound by any field's range; only the value
assigned to a field must fit. A division by zero is a model fault.

## Primary forms

```zin
balance                       # a field or a parameter
5   -3   true   closed        # literals
"on"                          # a quoted enum value
(balance + 1)
at of w        at[w]          # a map entry
status of order               # a record member
paid_on[c][tue]               # nested map entries
if c then a else b            # a conditional value
every w in worker satisfies at of w is done
some w in worker satisfies at of w is holding
number of w in worker where at of w is done
size of inside                # sets
length of queue   first of queue   last of queue   rest of queue    # lists
nothing                       # the absent optional value
{status: paid, items: 2}      # a record or map literal
[ann, ben]   []               # a set or list literal
running with {attempt: 2}     # an enumeration case with members
```

### Reading paths

`of` reads right to left: `status of orders of c` is the entry `c` of
`orders`, then its member `status`. Brackets read left to right and are
handy for maps of maps: `paid_on[c][tue]`. The types decide whether `x of y`
is a map entry or a record member.

### If-then-else

```zin
status becomes if attempts is 3 then failed else queued
lease becomes if left of lease is 1 then nothing else {holder: holder of lease, left: left of lease - 1}
```

Both branches must have the same type (an optional may mix a value and
`nothing`). Only the branch taken is evaluated.

```console
$ zin check ifty.zin
spec: ifty.zin:7:3: both branches of 'if' must have the same kind of value, but one is a number and the other a true/false value
```

`if` chains: `if ok then paid else if retryable then new else declined`.

## Quantifiers

```zin
every w in worker satisfies at of w is done
some w in worker satisfies at of w is holding
number of w in worker where at of w is done
every m in queue satisfies log has m
```

| Form | Value |
| --- | --- |
| `every x in R satisfies c` | true when `c` holds for every `x` (true when `R` is empty) |
| `some x in R satisfies c` | true when `c` holds for at least one `x` |
| `number of x in R where c` | how many `x` satisfy `c`, an integer |

`R` is a named type (an enumeration, a boolean, or an integer range declared
under `types`), a set field (its members) or a list field (its items, in
order). To quantify over numbers, name the range:

```zin
types
  small is an integer from 1 to 3
always every n in small satisfies ...
```

A quantifier extends **as far to the right as possible**. `every v in w
satisfies not done of v and flag` includes `and flag` in the body. To combine
a quantifier with something else, parenthesize it:

```zin
always (every v in w satisfies not done of v) and flag
always not open or (number of v in voter where vote of v is yes) <= 3
```

Quantifiers nest:

```zin
never
  some a in worker satisfies some b in worker satisfies a is not b and at of a is critical and at of b is critical
```

## Typing

Expressions are statically typed. Integers, booleans and each enumeration are
distinct types, and errors are reported before anything runs, even inside
actions that are never enabled:

```console
$ zin check mixed.zin
spec: mixed.zin:8:8: cannot compare a number with an enum value
  = see: zin explain type.mismatch
```

A map key must be of the map's key type. An enumeration that merely *contains*
the key values is not enough:

```console
$ zin check balancer.zin
spec: balancer.zin:10:28: load is keyed by one of s1, s2, s3, but the key here is s1 or s2 or s3 or none
  = see: zin explain type.mismatch
```

Fix it by testing the case and quantifying, or by making the field optional
(`draining is a server or nothing`) and guarding the read.

## Evaluation order and short-circuiting

Evaluation is left to right and stops as soon as the result is known:

- `and`, `or`, `implies` skip their right side once the left decides;
- `if` evaluates one branch;
- `some` stops at its first witness and `every` at its first counterexample,
  in the type's declared order;
- `or else` evaluates its default only when it is used.

A fault (division by zero, a key outside the map's key type, a read of an
absent value) counts only in a part that is actually evaluated. So a guard
on the left protects the right:

```zin
system "Work queue"
types
  worker is one of ann, ben
state
  load is a map from worker to an integer from 0 to 2
initially
  load of every worker is 0
  load of ben is 2
always every w in worker satisfies load of w is 0 or 2 / load of w is 1
```

```console
$ zin check queue.zin
ok Work queue: 1 state, 0 transitions, 1 property, 0 scenarios
```

Without the guard, the same property faults:

```console
$ zin check queue-unguarded.zin
model: queue-unguarded.zin:9:1: division by zero while evaluating 'every w in worker satisfies 2 / load of w is 1'
counterexample:
  start: load={ann: 0, ben: 2}
```

## Defaults with `or else`

`read or else default` is `read`, or `default` where the read finds no value:

| Read | Absent when |
| --- | --- |
| `last or else ann` | `last` is nothing |
| `left of lease or else 0` | `lease` is nothing |
| `attempt of job or else 0` | `job` is in a case without `attempt` |
| `first of queue or else ping` | `queue` is empty (also `last of`) |
| `rest of queue or else []` | `queue` is empty |

- **It binds tighter than arithmetic**, and the default is a single primary
  form or a negative number: `left or else 0 + 1` is `(left or else 0) + 1`.
  Parenthesize a computed default: `left of lease or else (limit - 1)`.
- **The default has the read's type**, and is evaluated only when used.
- **Only absence falls back.** Overflow, division by zero or a bad map key
  inside `read` is still a fault.
- **The read must be able to be absent.** A default for a value that is
  always there is an error:

  ```console
  $ zin check else-bad.zin
  spec: else-bad.zin:7:3: 'or else' gives a default for a value that can be absent: the value of an optional, a member of an enumeration case, or first, last or rest of a list; balance is always there
  ```

## Guards the checker recognizes

After a successful check, `zin check` notes each read that could find no
value and that nothing guards, even when no reachable state triggers it. See
[unguarded reads](diagnostics.md#unguarded-reads). A read is guarded when a
condition evaluated before it establishes the value is there:

- `lease is not nothing`, `last is w`;
- `job is running`, or `job is not idle` for every case without the member;
- `queue is not []`, `length of queue > 0`, `queue has m`.

The condition counts when it is an earlier `requires` of the same action, the
left side of `and` or `implies`, the left side of `or` (which must then be
false for the right to run), or the condition of an `if` (negated for the
`else` branch). Conditions are matched by the text of what they read, so a
guard on `lease of w` does not cover `lease of v`.
