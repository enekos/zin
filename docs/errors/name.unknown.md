# `name.unknown`

A name that nothing declares.

Every name in an expression must be a state field, a map, a member of a
record, an action parameter, a quantifier's variable, an enum value or a
named type. A typo is the usual cause. When a declared name of the right kind
is close, the error suggests it and `zin check --fix` can apply it. An enum
value that reads as a keyword (`on`, `nothing`) must be quoted.

## Example

```zin
system "Light"
types
  setting is one of off, dim, bright
state
  light is a setting
initially
  light is off
action brighten
  requires light is dmi
  light becomes bright
```

```text
spec: line 9: unknown name dmi; declare it or use a quoted enum value
  hint: did you mean `dim`?
    9 |   requires light is dim
      |                     ^^^
  = see: zin explain name.unknown
```

## Fix

Use the declared name.

```zin
system "Light"
types
  setting is one of off, dim, bright
state
  light is a setting
initially
  light is off
action brighten
  requires light is dim
  light becomes bright
```
