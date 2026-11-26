# `component.type_conflict`

Two system blocks declare a type under one name with different values.

Blocks may share a named type, so that a shared action's parameter means the
same thing in each, but every block that declares it must declare it the same
way. Declare it identically, or give the two types different names.

## Example

```zin
system "Shop"
types
  item is one of apple, pear
state
  stocked is a map from item to a boolean
initially
  stocked of every item is true
action sell(i is an item)
  requires stocked of i
  stocked of i becomes false

system "Till"
types
  item is one of apple, pear, plum
state
  sold is an integer from 0 to 3
initially
  sold is 0
action sell(i is an item)
  requires sold < 3
  sold becomes sold + 1
```

```text
spec: line 14: Till declares type item differently; components that share a type must declare it the same way (first declared on line 3)
  = see: zin explain component.type_conflict
```

## Fix

Declare the type the same way in both blocks.

```zin
system "Shop"
types
  item is one of apple, pear
state
  stocked is a map from item to a boolean
initially
  stocked of every item is true
action sell(i is an item)
  requires stocked of i
  stocked of i becomes false

system "Till"
types
  item is one of apple, pear
state
  sold is an integer from 0 to 3
initially
  sold is 0
action sell(i is an item)
  requires sold < 3
  sold becomes sold + 1
```
