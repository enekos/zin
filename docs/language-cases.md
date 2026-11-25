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

## Records

```zin
system "Order"
state
  order is a record with (status is one of draft, paid, items is an integer from 0 to 2)
initially
  order is {items: 0, status: draft}
action add
  requires status of order is draft and items of order < 2
  items of order becomes items of order + 1
action pay
  requires items of order > 0
  order becomes {status: paid, items: items of order}
scenario "Pay for one"
  add
  pay
  expect order is {status: paid, items: 1} # fails: expect order is {status: paid, items: 0}
```

## Optional values

```zin
system "Question"
state
  asked is an integer from 1 to 3 or nothing
initially
  asked is nothing
action ask(n is an integer from 1 to 3)
  requires asked is nothing
  asked becomes n
action answer
  requires asked is not nothing
  asked becomes nothing
always asked is nothing or asked > 0 # fails: always asked > 0
```

## Sets

```zin
system "Room"
types
  worker is one of ann, ben, cid
state
  inside is a set of worker
initially
  inside is []
action enter(w is a worker)
  requires not inside has w and size of inside < 2
  inside becomes inside with w
action leave(w is a worker)
  requires inside has w
  inside becomes inside without w
always size of inside <= 2 # fails: always size of inside <= 1
```

## Lists

```zin
system "Queue"
types
  message is one of ping, pong
state
  queue is a list of at most 2 message
initially
  queue is []
action send(m is a message)
  requires length of queue < 2
  queue becomes queue with m
action deliver
  requires queue is not []
  queue becomes rest of queue
scenario "First in, first out"
  send(ping)
  send(pong)
  deliver
  expect queue is [pong] # fails: expect queue is [ping]
```

## Enumeration values with members

```zin
system "Attempts"
state
  job is one of idle, running with (attempt is an integer from 1 to 2)
initially
  job is idle
action start
  requires job is idle
  job becomes running with {attempt: 1}
action retry
  requires job is running and attempt of job < 2
  job becomes running with {attempt: attempt of job + 1}
action stop
  requires job is running
  job becomes idle
always job is idle or attempt of job >= 1 # fails: always attempt of job >= 1
```
