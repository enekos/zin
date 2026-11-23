const commaSeparated = (rule) => seq(rule, repeat(seq(",", rule)));

module.exports = grammar({
  name: "zin",

  word: ($) => $.identifier,

  extras: ($) => [/[ \t]+/, $.comment],

  conflicts: ($) => [[$.enum_type]],

  rules: {
    source_file: ($) =>
      seq(
        repeat(choice($._newline, seq($._statement, $._newline))),
        optional($._statement),
      ),

    _statement: ($) =>
      choice(
        $.include_declaration,
        $.system_declaration,
        $.explore_declaration,
        $.section,
        $.property,
        $.whenever_property,
        $.terminal_property,
        $.action_declaration,
        $.requires,
        $.assignment,
        $.scenario_declaration,
        $.scenario_expect,
        $.scenario_reject,
        $.scenario_call,
        $.declaration,
        $.expression_statement,
      ),

    include_declaration: ($) => seq("include", field("path", $.string)),

    system_declaration: ($) => seq("system", field("name", $.string)),

    explore_declaration: ($) =>
      seq("explore", "at", "most", field("states", $.number), "states"),

    section: () => choice("types", "state", "initially"),

    property: ($) =>
      seq(
        choice("always", "never", "sometimes", "eventually"),
        optional(choice(seq("possible", $._expression), $._expression)),
      ),

    whenever_property: ($) =>
      seq(
        "whenever",
        field("premise", $._expression),
        "eventually",
        field("condition", $._expression),
      ),

    terminal_property: ($) => seq("terminal", "when", $._expression),

    action_declaration: ($) =>
      seq(
        optional(choice("fair", seq("strongly", "fair"))),
        "action",
        field("name", $.identifier),
        optional(field("parameters", $.parameter_list)),
      ),

    parameter_list: ($) =>
      seq("(", optional(commaSeparated($.parameter)), ")"),

    parameter: ($) =>
      seq(field("name", $.identifier), "is", field("type", $._type)),

    requires: ($) => seq("requires", $._expression),

    assignment: ($) =>
      seq(
        field("target", $._assignment_target),
        "becomes",
        field("value", $._expression),
      ),

    scenario_declaration: ($) => seq("scenario", field("name", $.string)),

    scenario_expect: ($) => seq("expect", $._expression),

    scenario_reject: ($) => seq("reject", $.scenario_call),

    scenario_call: ($) =>
      prec(
        1,
        seq(
          field("action", $.identifier),
          optional(
            choice(
              field("arguments", $.argument_list),
              seq("with", commaSeparated($.named_argument)),
            ),
          ),
        ),
      ),

    argument_list: ($) =>
      seq("(", optional(commaSeparated($._expression)), ")"),

    named_argument: ($) =>
      seq(field("name", $.identifier), "=", field("value", $._expression)),

    declaration: ($) =>
      prec(
        1,
        seq(
          optional("interchangeable"),
          field("name", $._assignment_target),
          "is",
          field(
            "value",
            choice(
              $.integer_type,
              $.boolean_type,
              $.enum_type,
              $.map_type,
              $.prefixed_named_type,
              $._expression,
            ),
          ),
        ),
      ),

    expression_statement: ($) => prec(-1, $._expression),

    _type: ($) =>
      choice(
        $.integer_type,
        $.boolean_type,
        $.enum_type,
        $.map_type,
        $.named_type,
      ),

    integer_type: ($) =>
      seq(
        "an",
        "integer",
        "from",
        field("minimum", $._expression),
        "to",
        field("maximum", $._expression),
      ),

    boolean_type: () => seq("a", "boolean"),

    enum_type: ($) =>
      seq(
        "one",
        "of",
        $.enum_value,
        repeat(seq(choice(",", "or"), $.enum_value)),
      ),

    enum_value: ($) => choice($.identifier, $.string),

    map_type: ($) =>
      seq("a", "map", "from", field("key", $._type), "to", field("value", $._type)),

    named_type: ($) => seq(optional("a"), field("name", $.identifier)),

    prefixed_named_type: ($) => seq("a", field("name", $.identifier)),

    _assignment_target: ($) => prec(2, choice($.identifier, $.map_access)),

    _expression: ($) =>
      choice(
        $.binary_expression,
        $.unary_expression,
        $.quantified_expression,
        $.count_expression,
        $.conditional_expression,
        $._primary,
      ),

    binary_expression: ($) =>
      prec.left(
        1,
        seq(
          field("left", $._expression),
          field("operator", $.operator),
          field("right", $._expression),
        ),
      ),

    operator: () =>
      choice(
        "implies",
        "or",
        "and",
        "is not",
        "is different from",
        "is less than",
        "is at most",
        "is greater than",
        "is more than",
        "is at least",
        "is",
        "=",
        "==",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "equals",
        "+",
        "-",
        "*",
        "/",
        "plus",
        "minus",
        "times",
        "divided by",
      ),

    unary_expression: ($) =>
      prec(2, seq(field("operator", choice("not", "-")), field("value", $._expression))),

    quantified_expression: ($) =>
      prec.right(
        seq(
          field("quantifier", choice("some", "every")),
          field("name", $.identifier),
          "in",
          field("collection", $._expression),
          "satisfies",
          field("condition", $._expression),
        ),
      ),

    count_expression: ($) =>
      prec.right(
        seq(
          "number",
          "of",
          field("name", $.identifier),
          "in",
          field("collection", $._expression),
          "where",
          field("condition", $._expression),
        ),
      ),

    conditional_expression: ($) =>
      prec.right(
        seq(
          "if",
          field("condition", $._expression),
          "then",
          field("consequence", $._expression),
          "else",
          field("alternative", $._expression),
        ),
      ),

    _primary: ($) =>
      choice(
        $.number,
        $.string,
        $.boolean,
        $.identifier,
        $.map_access,
        $.parenthesized_expression,
      ),

    map_access: ($) =>
      seq(
        field("collection", $.identifier),
        choice(
          seq(
            "of",
            choice(
              seq("every", field("type", $.identifier)),
              field("key", $._primary),
            ),
          ),
          seq("[", field("key", $._expression), "]"),
        ),
      ),

    parenthesized_expression: ($) => seq("(", $._expression, ")"),

    boolean: () => choice("true", "false"),

    number: () => /[0-9]+/,

    identifier: () => /[A-Za-z_][A-Za-z0-9_]*/,

    string: ($) => seq('"', repeat(choice($.string_content, $.escape_sequence)), '"'),

    string_content: () => /[^"\\\r\n]+/,

    escape_sequence: () => /\\./,

    comment: () => /#[^\r\n]*/,

    _newline: () => /\r?\n/,
  },
});
