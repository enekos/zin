# Executable language examples

Each specification below passes `zin check`. Replacing the line before a
`# fails:` comment with the text after it produces a counterexample or a
diagnostic. The language tests check both versions.

## Precedence, scope and short-circuiting

```zin
system "Work queue"
types
  worker is one of ann, ben
state
  load is a map from worker to an integer from 0 to 2
initially
  load of every worker is 0
  load of ben is 2
always (some w in worker satisfies load of w is 2) and 2 + 1 * 2 is 4
always every w in worker satisfies load of w is 0 or 2 / load of w is 1 # fails: always every w in worker satisfies 2 / load of w is 1
```

## Simultaneous writes and map overrides

```zin
system "Swap"
types
  key is one of first, second
state
  values is a map from key to an integer from 0 to 2
initially
  values of every key is 1
  values of second is 2
action swap
  values of first becomes values of second
  values of second becomes values of first
scenario "One swap"
  swap
  expect values of first is 2 and values of second is 1 # fails: expect values of first is 2 and values of second is 2
```

## Fairness per call

```zin
system "Delivery"
types
  item is one of a, b
state
  sent is a map from item to a boolean
initially
  sent of every item is false
fair action send(i is an item) # fails: action send(i is an item)
  sent of i becomes true
eventually every i in item satisfies sent of i
```

## Shared parameters

```zin
system "Client"
state
  sent is a boolean
initially
  sent is false
action send(value is a boolean)
  sent becomes value
system "Server"
state
  received is a boolean
initially
  received is false
action send(value is a boolean) # fails: action send(value is an integer from 0 to 1)
  received becomes value
always sent is received
```
