(identifier) @variable

(comment) @comment
(string) @string
(escape_sequence) @string.escape
(number) @number
(boolean) @boolean
(operator) @operator

(system_declaration
  name: (string) @string.special)

(include_declaration
  path: (string) @string.special)

(scenario_declaration
  name: (string) @string.special)

(action_declaration
  name: (identifier) @function)

(scenario_call
  action: (identifier) @function)

(parameter
  name: (identifier) @variable.parameter)

(named_argument
  name: (identifier) @variable.parameter)

(named_argument
  "=" @operator)

(declaration
  name: (identifier) @property)

(assignment
  target: (identifier) @property)

(map_access
  collection: (identifier) @property)

(map_access
  type: (identifier) @type)

(named_type
  name: (identifier) @type)

(prefixed_named_type
  name: (identifier) @type)

(quantified_expression
  name: (identifier) @variable.parameter)

(quantified_expression
  collection: (identifier) @type)

(count_expression
  name: (identifier) @variable.parameter)

(count_expression
  collection: (identifier) @type)

(enum_value
  (identifier) @constant)

(integer_type
  "integer" @type.builtin)

(boolean_type
  "boolean" @type.builtin)

(map_type
  "map" @type.builtin)

[
  "include"
  "system"
  "explore"
  "at"
  "most"
  "states"
  "types"
  "state"
  "initially"
  "interchangeable"
  "always"
  "never"
  "sometimes"
  "eventually"
  "possible"
  "whenever"
  "terminal"
  "when"
  "fair"
  "strongly"
  "action"
  "requires"
  "becomes"
  "scenario"
  "expect"
  "reject"
  "with"
  "a"
  "an"
  "from"
  "to"
  "one"
  "of"
  "not"
  "some"
  "every"
  "number"
  "in"
  "where"
  "satisfies"
  "if"
  "then"
  "else"
] @keyword

[
  "("
  ")"
  "["
  "]"
] @punctuation.bracket

"," @punctuation.delimiter
