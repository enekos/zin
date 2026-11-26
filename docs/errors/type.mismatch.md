# `type.mismatch`

A value of one type where another is needed.

Integers, booleans and each enumeration are distinct types, and the checker
rejects mixing them before anything runs, even in an action that is never
enabled. Conditions need booleans; arithmetic and ordering need integers;
`is` compares values of the same type.

## Example

```zin
system "Counter"
state
  count is an integer from 0 to 3
  done is a boolean
initially
  count is 0
  done is false
action finish
  requires count
  done becomes true
```

```text
spec: line 9: 'count' must be true or false, but it is a number
  = see: zin explain type.mismatch
```

## Fix

Say which integers enable the action.

```zin
system "Counter"
state
  count is an integer from 0 to 3
  done is a boolean
initially
  count is 0
  done is false
action finish
  requires count > 0
  done becomes true
```
