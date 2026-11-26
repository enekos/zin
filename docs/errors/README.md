# Diagnostic codes

Every error `zin` reports carries one of these codes: `zin check --format json`
reports it as `code`, Rust as `Error::code`, and the text output ends with
`= see: zin explain <code>` when a finer code than the error's kind applies.
`zin explain <code>` prints the page; each page shows a specification that
gets the error, the exact output, and a fix. The test suite runs every
example.

| Code | Meaning | Since |
| --- | --- | --- |
| [`spec`](spec.md) | The specification cannot be read: a syntax error or a rule of the language it breaks. | 0.3.0 |
| [`usage`](usage.md) | The command line or a file it names cannot be used. | 0.3.0 |
| [`model.fault`](model.fault.md) | Some reachable step leaves a declared range, divides by zero or reads a value that is not there. | 0.3.0 |
| [`violation`](violation.md) | A property does not hold; the counterexample shows how. | 0.3.0 |
| [`limit`](limit.md) | Exploration stopped at a limit before it could decide. | 0.3.0 |
| [`name.unknown`](name.unknown.md) | A name that nothing declares. | 0.3.0 |
| [`type.mismatch`](type.mismatch.md) | A value of one type where another is needed. | 0.3.0 |
| [`component.field_conflict`](component.field_conflict.md) | Two system blocks declare the same state field. | 0.3.0 |
| [`component.duplicate_action`](component.duplicate_action.md) | One system block declares an action twice. | 0.3.0 |
| [`component.foreign_field`](component.foreign_field.md) | A system block touches a field that another block owns. | 0.3.0 |
| [`component.type_conflict`](component.type_conflict.md) | Two system blocks declare a type under one name with different values. | 0.3.0 |
| [`component.parameter_conflict`](component.parameter_conflict.md) | Two system blocks give a shared action's parameter different types. | 0.3.0 |

New codes may be added; a code is never reused for a different meaning.
