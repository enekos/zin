# `model.fault`

Some reachable step leaves a declared range, divides by zero or reads a value that is not there.

The specification is well typed, but some reachable state lets an action
compute a value it cannot hold: an integer outside its declared range, a
division by zero, the value of `nothing`, a member of a case that does not
hold, or an item of an empty list. The trace shows the shortest way there.
Add a `requires` that rules the step out, or guard the read.

## Example

```zin
system "Tank"
state
  level is an integer from 0 to 2
initially
  level is 0
action fill
  level becomes level + 1
```

```text
model: line 7: fill: level would become 3, but it is an integer from 0 to 2; add a 'requires' that rules this out
counterexample:
  start: level=0
  1. fill  ->  level=1
  2. fill  ->  level=2
```

## Fix

Refuse to fill a full tank.

```zin
system "Tank"
state
  level is an integer from 0 to 2
initially
  level is 0
action fill
  requires level < 2
  level becomes level + 1
```
