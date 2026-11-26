# `component.duplicate_action`

One system block declares an action twice.

Within a block an action is declared once. Across blocks the same name is
one shared action that steps all of them together, so the error only arises
inside one block. Merge the two declarations, or rename one.

## Example

```zin
system "Lamp"
state
  on is a boolean
initially
  on is false
action press
  requires not on
  on becomes true
action press
  requires on
  on becomes false

system "Counter"
state
  presses is an integer from 0 to 2
initially
  presses is 0
action press
  requires presses < 2
  presses becomes presses + 1
```

```text
spec: line 9: duplicate action press in Lamp (first declared on line 6)
  = see: zin explain component.duplicate_action
```

## Fix

One action can say both things.

```zin
system "Lamp"
state
  on is a boolean
initially
  on is false
action press
  on becomes not on

system "Counter"
state
  presses is an integer from 0 to 2
initially
  presses is 0
action press
  requires presses < 2
  presses becomes presses + 1
```
