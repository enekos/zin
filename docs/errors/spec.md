# `spec`

The specification cannot be read: a syntax error or a rule of the language it breaks.

This is the code of every specification error that has no finer code yet.
The message names the rule and the location points at the token or statement
that breaks it. [The language reference](../language.md) states each rule.

## Example

```zin
system "Door"
state
  open is a boolean
initially
  open is false
action toggle
  open becomes not open
  open becomes false
```

```text
spec: line 8: this value is already assigned in the same block
```

## Fix

Assign each variable at most once per action.

```zin
system "Door"
state
  open is a boolean
initially
  open is false
action toggle
  open becomes not open
```
