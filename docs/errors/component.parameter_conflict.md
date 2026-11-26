# `component.parameter_conflict`

Two system blocks give a shared action's parameter different types.

A shared action's parameters with the same name are the same value, which is
how a shared action passes data between components, so they need the same
type. Give the parameters one type, or different names if they are
different values.

## Example

```zin
system "Sender"
state
  last is an integer from 0 to 3
initially
  last is 0
action send(n is an integer from 0 to 3)
  last becomes n

system "Receiver"
state
  got is an integer from 0 to 3
initially
  got is 0
action send(n is an integer from 1 to 3)
  got becomes n
```

```text
spec: line 14: Receiver gives parameter n of the shared action send the type an integer from 1 to 3, but another component gives it an integer from 0 to 3 (first declared on line 6)
  = see: zin explain component.parameter_conflict
```

## Fix

Use one range for the value both sides see.

```zin
system "Sender"
state
  last is an integer from 0 to 3
initially
  last is 0
action send(n is an integer from 0 to 3)
  last becomes n

system "Receiver"
state
  got is an integer from 0 to 3
initially
  got is 0
action send(n is an integer from 0 to 3)
  got becomes n
```
