# `limit`

Exploration stopped at a limit before it could decide.

The checker never reports a partial answer: past the declared state limit
(`explore at most N states`, one million by default) or 20 million
transitions it stops with this error. Raise the limit if the model is meant
to be that large, or shrink the ranges: a model usually needs only a few
values of each kind to show its behaviour.

## Example

```zin
system "Clock"
explore at most 10 states
state
  minutes is an integer from 0 to 59
initially
  minutes is 0
action tick
  minutes becomes (minutes + 1) - 60 * ((minutes + 1) / 60)
```

```text
limit: more than 10 states are reachable; tighten the ranges or raise the limit with 'explore at most N states'
```

## Fix

Make room for every reachable state.

```zin
system "Clock"
explore at most 100 states
state
  minutes is an integer from 0 to 59
initially
  minutes is 0
action tick
  minutes becomes (minutes + 1) - 60 * ((minutes + 1) / 60)
```
