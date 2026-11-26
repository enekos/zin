# `component.foreign_field`

A system block touches a field that another block owns.

An action or initial value in one block may read and write only that block's
fields. Components affect each other only through shared actions: declare
the action in both blocks, each with the conditions and assignments on its
own fields.

## Example

```zin
system "Client"
state
  sent is a boolean
initially
  sent is false
action send
  requires not sent
  sent becomes true
  received becomes true

system "Server"
state
  received is a boolean
initially
  received is false
```

```text
spec: line 9: action send of Client assigns received, which belongs to Server; a component reaches another only through shared actions
  = see: zin explain component.foreign_field
```

## Fix

Let the server take part in `send`.

```zin
system "Client"
state
  sent is a boolean
initially
  sent is false
action send
  requires not sent
  sent becomes true

system "Server"
state
  received is a boolean
initially
  received is false
action send
  received becomes true
```
