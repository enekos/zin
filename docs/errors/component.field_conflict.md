# `component.field_conflict`

Two system blocks declare the same state field.

Components share no state: each field belongs to the block that declares it.
Rename one of the fields, and pass values between the components through a
shared action. The error also points at the first declaration.

## Example

```zin
system "Client"
state
  status is one of idle, waiting
initially
  status is idle
action send
  status becomes waiting

system "Server"
state
  status is one of free, busy
initially
  status is free
action send
  status becomes busy
```

```text
spec: line 11: status is already a state field of Client; components share no state, so rename one and pass values through a shared action (first declared on line 3)
  = see: zin explain component.field_conflict
```

## Fix

Give each component its own field.

```zin
system "Client"
state
  client is one of idle, waiting
initially
  client is idle
action send
  client becomes waiting

system "Server"
state
  server is one of free, busy
initially
  server is free
action send
  server becomes busy
```
