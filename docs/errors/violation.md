# `violation`

A property does not hold; the counterexample shows how.

`always` and `never` fail in a reachable state, `sometimes` in none, `always
possible` from a state with no way back, and `eventually` on a run that
avoids its condition forever (the counterexample then ends with `then
forever:`). Either the specification allows something it should not, or the
property asks too much. Decide which, then add the missing `requires` or
correct the property.

## Example

```zin
system "Account"
state
  balance is an integer from 0 to 3
  closed is a boolean
initially
  balance is 0
  closed is false
always closed implies balance is 0
action deposit
  requires not closed and balance < 3
  balance becomes balance + 1
action close
  closed becomes true
```

```text
violation: line 8: 'always closed implies balance is 0' is violated
counterexample:
  start: balance=0, closed=false
  1. deposit  ->  balance=1
  2. close  ->  closed=true
```

## Fix

Close only an empty account.

```zin
system "Account"
state
  balance is an integer from 0 to 3
  closed is a boolean
initially
  balance is 0
  closed is false
always closed implies balance is 0
action deposit
  requires not closed and balance < 3
  balance becomes balance + 1
action close
  requires balance is 0
  closed becomes true
```
