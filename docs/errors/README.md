# Diagnostic codes

Every error `zin` reports carries one of these codes: `zin check --format json`
reports it as `code`, Rust as `Error::code`, and the text output ends with
`= see: zin explain <code>` when a finer code than the error's kind applies.
`zin explain <code>` prints the page; each page shows a specification that
gets the error, the exact output, and a fix. The test suite runs every
example.

| Code | Meaning |
| --- | --- |
| [`spec`](spec.md) | The specification cannot be read: a syntax error or a rule of the language it breaks. |
| [`usage`](usage.md) | The command line or a file it names cannot be used. |
| [`model.fault`](model.fault.md) | Some reachable step leaves a declared range, divides by zero or reads a value that is not there. |
| [`violation`](violation.md) | A property does not hold; the counterexample shows how. |
| [`limit`](limit.md) | Exploration stopped at a limit before it could decide. |
| [`name.unknown`](name.unknown.md) | A name that nothing declares. |
| [`type.mismatch`](type.mismatch.md) | A value of one type where another is needed. |
| [`component.field_conflict`](component.field_conflict.md) | Two system blocks declare the same state field. |
| [`component.duplicate_action`](component.duplicate_action.md) | One system block declares an action twice. |
| [`component.foreign_field`](component.foreign_field.md) | A system block touches a field that another block owns. |
| [`component.type_conflict`](component.type_conflict.md) | Two system blocks declare a type under one name with different values. |
| [`component.parameter_conflict`](component.parameter_conflict.md) | Two system blocks give a shared action's parameter different types. |

New codes may be added; a code is never reused for a different meaning.
