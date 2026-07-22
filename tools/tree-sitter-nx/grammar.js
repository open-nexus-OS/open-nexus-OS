/**
 * Tree-sitter grammar for the `.nx` DSL (Open Nexus OS).
 *
 * CONTEXT: Structural syntax for editors — highlighting, folding, indent,
 *          text objects. Not a validator: `nx-dsl lint` remains the authority
 *          on what is a legal program.
 * OWNERS:  @tools-team
 * STATUS:  Functional
 * SSOT:    userspace/dsl/core/src/{lexer.rs,parser/*.rs}
 *          docs/dev/dsl/grammar.md (normative prose)
 *
 * Where the two disagree, this grammar follows the COMPILER, because the
 * compiler is what actually accepts or rejects a file. Deltas found while
 * writing this (present in the parser, absent from grammar.md):
 *
 *   - `Component` may carry a `state: { ... }` block after `props: { ... }`
 *     (parser/decls.rs component_decl)
 *   - `Window { style:, mode:, level:, resizable: }` declarations
 *     (parser/decls.rs window_decl)
 *   - handler action `navigate(expr)` alongside `dispatch` / `emit`
 *     (parser/view.rs handler)
 *   - handlers may appear INSIDE a widget's brace body, not only after it
 *   - `Query` is a contextual keyword: an ordinary identifier everywhere
 *     except in declaration position (parser/mod.rs decl)
 *
 * Bounds from the lexer that are NOT enforced here (they are semantic limits,
 * and a grammar that rejected them would break editing rather than help):
 * MAX_FILE_BYTES, MAX_IDENT_BYTES, MAX_NESTING, integer/Fx range.
 */

/// Precedence ladder, loose -> tight. Mirrors parser/expr.rs.
const PREC = {
  or: 1,
  and: 2,
  compare: 3,
  add: 4,
  multiply: 5,
  unary: 6,
};

/**
 * A name in type/widget/case position.
 *
 * The DSL has exactly ONE identifier token — capitalisation is a convention
 * the compiler never checks. So this is an alias, not a separate token: a
 * second token with the same pattern would be a lexical conflict wherever
 * both are valid (for example inside a widget body, where `foo:` is a
 * property and `Foo {` is a child widget).
 */
function typeName($) {
  return alias($.identifier, $.type_identifier);
}

module.exports = grammar({
  name: 'nx',

  // Comments and whitespace are insignificant outside string literals.
  // The lexer has line comments only — there is no block-comment form.
  extras: ($) => [/\s/, $.comment],

  // Keyword extraction: lex a whole word, then decide whether it is a keyword
  // *in this parse state*. This is what makes `Query`, `params`, `where`,
  // `orderBy`, `limit`, `asc` and `desc` contextual for free — they stay
  // usable as ordinary names outside the positions that expect them.
  word: ($) => $.identifier,

  rules: {
    // File = { Import } { Decl } — imports must precede declarations.
    source_file: ($) => seq(repeat($.import), repeat($._declaration)),

    import: ($) => seq('import', field('path', $.string_literal)),

    _declaration: ($) =>
      choice(
        $.store_declaration,
        $.event_declaration,
        $.reduce_declaration,
        $.effect_declaration,
        $.component_declaration,
        $.page_declaration,
        $.routes_declaration,
        $.window_declaration,
        $.query_declaration,
      ),

    // ---------------------------------------------------------------- state

    // Store Name { field: Type = default @persist, ... }
    store_declaration: ($) =>
      seq('Store', field('name', typeName($)), '{', repeat($.store_field), '}'),

    store_field: ($) =>
      seq(
        field('name', $.identifier),
        ':',
        field('type', $.type),
        optional(seq('=', field('default', $._expression))),
        // `@persist` comes AFTER the default value (parser/decls.rs).
        optional(field('persist', $.persist_attribute)),
        ',',
      ),

    persist_attribute: (_) => '@persist',

    // Event Name { Case, Case(Type, ...), ... }
    event_declaration: ($) =>
      seq('Event', field('name', typeName($)), '{', repeat($.event_case), '}'),

    event_case: ($) =>
      seq(
        field('name', typeName($)),
        optional(seq('(', commaSep1(field('payload', $.type)), ')')),
        ',',
      ),

    // reduce EventName { Case => stmt-or-block, ... }
    reduce_declaration: ($) =>
      seq('reduce', field('event', typeName($)), '{', repeat1($.reduce_arm), '}'),

    reduce_arm: ($) =>
      seq(field('pattern', $.pattern), '=>', field('body', $._statement_or_block), ','),

    // @effect on Case(binds) { stmts }
    effect_declaration: ($) =>
      seq('@effect', 'on', field('trigger', $.pattern), field('body', $.block)),

    // CaseName / CaseName(bind, ...)
    pattern: ($) =>
      seq(
        field('case', typeName($)),
        optional(seq('(', commaSep1(field('binding', $.identifier)), ')')),
      ),

    // ---------------------------------------------------------------- views

    // Page Name { <exactly one view node> }
    page_declaration: ($) =>
      seq('Page', field('name', typeName($)), '{', field('view', $._view_node), '}'),

    // Component Name { [props: {...}] [state: {...}] <one view node> }
    component_declaration: ($) =>
      seq(
        'Component',
        field('name', typeName($)),
        '{',
        optional($.props_block),
        optional($.state_block),
        field('view', $._view_node),
        '}',
      ),

    props_block: ($) => seq('props', ':', '{', repeat($.prop_declaration), '}'),

    prop_declaration: ($) => seq(field('name', $.identifier), ':', field('type', $.type), ','),

    // Component-local state. Not in grammar.md; see parser/decls.rs.
    state_block: ($) => seq('state', ':', '{', repeat($.state_field), '}'),

    state_field: ($) =>
      seq(
        field('name', $.identifier),
        ':',
        field('type', $.type),
        optional(seq('=', field('default', $._expression))),
        ',',
      ),

    _view_node: ($) => choice($.if_view, $.for_view, $.match_view, $.widget),

    // `if c { .. } else if c { .. } else { .. }`
    if_view: ($) =>
      seq(
        'if',
        field('condition', $._expression),
        field('consequence', $.view_block),
        repeat(
          seq('else', 'if', field('condition', $._expression), field('consequence', $.view_block)),
        ),
        optional(seq('else', field('alternative', $.view_block))),
      ),

    for_view: ($) =>
      seq(
        'for',
        field('binding', $.identifier),
        'in',
        field('iterable', $._expression),
        field('body', $.view_block),
      ),

    match_view: ($) =>
      seq('match', field('value', $._expression), '{', repeat1($.match_view_arm), '}'),

    match_view_arm: ($) => seq(field('pattern', $.pattern), '=>', field('body', $.view_block), ','),

    view_block: ($) => seq('{', repeat($._view_node), '}'),

    /**
     * Widget or component instantiation:
     *
     *   Divider                               -- bare
     *   Text("Hi")                            -- positional sugar
     *   Stack { gap: 2, Text("a") }           -- props and children mixed
     *   List($state.rows) { row in Text(..) } -- collection template
     *   Panel { ... }.padding(4) on Tap -> dispatch(Open)
     *
     * `prec.right` is load-bearing. Inside a parent body, a trailing
     * `on Tap -> ...` could attach either to the widget just parsed or to the
     * enclosing body (both accept handlers):
     *
     *     Stack {
     *         Button { label: "-" }.bg(surface)
     *         on Tap -> dispatch(Dec)      <- Button's, not Stack's
     *     }
     *
     * parser/view.rs resolves this greedily (`while peek == KwOn`), binding
     * the handler to the inner widget. prec.right makes the LR parser shift
     * rather than reduce, which reproduces exactly that.
     */
    widget: ($) =>
      prec.right(
        seq(
          field('name', typeName($)),
          optional(seq('(', field('positional', $._expression), ')')),
          optional(field('body', $.widget_body)),
          repeat(field('modifier', $.modifier)),
          repeat(field('handler', $.handler)),
        ),
      ),

    /**
     * One brace body covers both the prop/children block and the collection
     * template. Keeping them as a single rule is deliberate: `{ row in ... }`
     * and `{ row: expr }` only diverge on the token AFTER the identifier, so
     * one merged rule resolves with a single lookahead, whereas two competing
     * rules would need a declared LR conflict.
     */
    widget_body: ($) => seq('{', optional($.item_binding), repeat($._body_entry), '}'),

    // `row in` — the loop variable of a collection template.
    item_binding: ($) => seq(field('name', $.identifier), 'in'),

    /**
     * A body entry is a property, a handler, or a child view node.
     *
     * The handler here carries an optional `,`/`;`: parser/view.rs eats one
     * after an *inline* handler (`let _ = self.eat(&Semi) || self.eat(&Comma)`)
     * but not after the trailing handlers of a widget. So
     *
     *     Stack { on Tap -> dispatch(X); Text("a") }   <- separator allowed
     *     Stack { Button {} on Tap -> dispatch(X) }    <- binds to Button, none
     *
     * are both correct, and `prec.right` on `widget` is what routes the second
     * case into the widget instead of here.
     */
    _body_entry: ($) =>
      choice($.property, seq($.handler, optional(choice(',', ';'))), $._view_node),

    // `name: expr` with an optional `,` or `;` separator.
    property: ($) =>
      seq(
        field('name', $.identifier),
        ':',
        field('value', $._expression),
        optional(choice(',', ';')),
      ),

    // `.name(args)` — only a modifier when an argument list follows; a bare
    // `.name` is field access on an expression instead.
    modifier: ($) =>
      seq('.', field('name', $.identifier), '(', optional(field('arguments', $.arguments)), ')'),

    // `on Tap -> dispatch(Case(args))` / `-> emit(prop, args)` / `-> navigate(path)`
    handler: ($) =>
      seq('on', field('trigger', typeName($)), '->', field('action', $._handler_action)),

    _handler_action: ($) => choice($.dispatch_action, $.emit_action, $.navigate_action),

    dispatch_action: ($) =>
      seq(
        'dispatch',
        '(',
        field('case', typeName($)),
        optional(seq('(', optional(commaSep1($._expression)), ')')),
        ')',
      ),

    emit_action: ($) =>
      seq('emit', '(', field('target', $._expression), repeat(seq(',', $._expression)), ')'),

    navigate_action: ($) => seq('navigate', '(', field('path', $._expression), ')'),

    // --------------------------------------------------------------- routes

    routes_declaration: ($) => seq('Routes', '{', repeat($.route), '}'),

    route: ($) =>
      seq(
        field('path', $.string_literal),
        '->',
        field('page', typeName($)),
        optional(seq('(', commaSep1($.route_parameter), ')')),
        ';',
      ),

    route_parameter: ($) => seq(field('name', $.identifier), ':', field('type', $.type)),

    // --------------------------------------------------------------- window

    // Window { style: plain, mode: fullscreen, level: desktop, resizable: false }
    window_declaration: ($) => seq('Window', '{', repeat($.window_field), '}'),

    window_field: ($) =>
      seq(
        field('name', $.identifier),
        ':',
        field('value', choice($.identifier, $.boolean_literal)),
        // Separator is optional on the last field.
        optional(choice(',', ';')),
      ),

    // ---------------------------------------------------------------- query

    // `Query` is contextual — only a keyword in declaration position.
    query_declaration: ($) =>
      seq(
        'Query',
        field('name', typeName($)),
        'on',
        field('source', $.identifier),
        '{',
        repeat($._query_clause),
        '}',
      ),

    _query_clause: ($) =>
      choice($.params_clause, $.where_clause, $.order_by_clause, $.limit_clause),

    params_clause: ($) => seq('params', ':', '{', repeat($.prop_declaration), '}', ','),

    where_clause: ($) =>
      seq(
        'where',
        field('column', $.identifier),
        field('operator', choice('==', '>=', '<=', '>', '<')),
        field('value', $._expression),
        ',',
      ),

    order_by_clause: ($) =>
      seq('orderBy', field('column', $.identifier), optional(choice('asc', 'desc')), ','),

    limit_clause: ($) => seq('limit', field('count', $.integer_literal), ','),

    // ----------------------------------------------------------- statements

    block: ($) => seq('{', repeat($._statement_in_block), '}'),

    // Inside a block every simple statement is `;`-terminated; `if` and
    // `match` are not.
    _statement_in_block: ($) =>
      choice(seq($._simple_statement, ';'), $.if_statement, $.match_statement),

    // A reducer/match arm may hold a single statement without its `;`,
    // because the arm's `,` already terminates it.
    _statement_or_block: ($) =>
      choice($.block, $._simple_statement, $.if_statement, $.match_statement),

    _simple_statement: ($) =>
      choice($.let_statement, $.dispatch_statement, $.assignment_statement, $.expression_statement),

    let_statement: ($) =>
      seq('let', field('name', $.identifier), '=', field('value', $._expression)),

    // `state.a.b = expr` / `+=` / `-=`
    assignment_statement: ($) =>
      seq(
        field('target', $.state_place),
        field('operator', choice('=', '+=', '-=')),
        field('value', $._expression),
      ),

    state_place: ($) => seq('state', repeat1(seq('.', field('field', $.identifier)))),

    dispatch_statement: ($) =>
      seq(
        'dispatch',
        '(',
        field('case', typeName($)),
        optional(seq('(', optional(commaSep1($._expression)), ')')),
        ')',
      ),

    // Only service calls stand alone as statements (parser/stmt.rs).
    expression_statement: ($) => $.service_call,

    if_statement: ($) =>
      seq(
        'if',
        field('condition', $._expression),
        field('consequence', $.block),
        optional(seq('else', field('alternative', choice($.if_statement, $.block)))),
      ),

    match_statement: ($) =>
      seq('match', field('value', $._expression), '{', repeat1($.match_statement_arm), '}'),

    match_statement_arm: ($) =>
      seq(field('pattern', $.pattern), '=>', field('body', $._statement_or_block), ','),

    // ---------------------------------------------------------- expressions

    _expression: ($) => choice($.binary_expression, $.unary_expression, $._primary_expression),

    binary_expression: ($) => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.compare, choice('==', '!=', '<', '<=', '>', '>=')],
        [PREC.add, choice('+', '-')],
        [PREC.multiply, choice('*', '/', '%')],
      ];
      return choice(
        ...table.map(([precedence, operator]) =>
          prec.left(
            precedence,
            seq(
              field('left', $._expression),
              field('operator', operator),
              field('right', $._expression),
            ),
          ),
        ),
      );
    },

    unary_expression: ($) =>
      prec(PREC.unary, seq(field('operator', choice('!', '-')), field('operand', $._expression))),

    _primary_expression: ($) =>
      choice(
        $.integer_literal,
        $.fixed_literal,
        $.string_literal,
        $.boolean_literal,
        $.list_literal,
        $.i18n_call,
        $.state_reference,
        $.props_reference,
        $.device_reference,
        $.service_call,
        $.enum_literal,
        $.call_expression,
        $.path_expression,
        $.parenthesized_expression,
      ),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    list_literal: ($) => seq('[', optional(commaSep1($._expression)), ']'),

    // `$state.a.b`, and the bare `state.a` form used inside reducer bodies.
    state_reference: ($) =>
      seq(choice('$state', 'state'), repeat1(seq('.', field('field', $.identifier)))),

    props_reference: ($) => seq('$props', repeat1(seq('.', field('field', $.identifier)))),

    // Read-only environment: `device.profile`, ...
    device_reference: ($) => seq('device', repeat1(seq('.', field('field', $.identifier)))),

    // `svc.service.method(args)` — arguments are mandatory.
    service_call: ($) =>
      seq(
        'svc',
        repeat1(seq('.', field('segment', $.identifier))),
        '(',
        optional(field('arguments', $.arguments)),
        ')',
      ),

    // `@t("key")` / `@t("key", arg, ...)`
    i18n_call: ($) =>
      seq('@t', '(', field('key', $.string_literal), repeat(seq(',', $._expression)), ')'),

    // `Type::Case` / `Type::Case(args)`
    enum_literal: ($) =>
      seq(
        field('type', typeName($)),
        '::',
        field('case', typeName($)),
        optional(seq('(', optional(commaSep1($._expression)), ')')),
      ),

    path_expression: ($) => seq($.identifier, repeat(seq('.', field('field', $.identifier)))),

    call_expression: ($) =>
      seq(field('function', $.path_expression), '(', optional(field('arguments', $.arguments)), ')'),

    // Positional and named arguments may be mixed (parser/expr.rs call_args).
    arguments: ($) => commaSep1(choice($.named_argument, $._expression)),

    named_argument: ($) => seq(field('name', $.identifier), ':', field('value', $._expression)),

    // ----------------------------------------------------------------- types

    // `Name` / `Name<T, U>` — nested generics lex fine because there is no
    // `>>` token in the lexer.
    type: ($) => seq(field('name', typeName($)), optional(seq('<', commaSep1($.type), '>'))),

    // --------------------------------------------------------------- lexical

    identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // Line comments only.
    comment: (_) => token(seq('//', /.*/)),

    // No raw newline may appear inside a string (the lexer treats it as
    // "unterminated"), and the escape set is closed: \n \t \" \\
    // token.immediate keeps `extras` from eating the spaces inside a string.
    string_literal: ($) =>
      seq('"', repeat(choice($.escape_sequence, token.immediate(/[^"\\\n]+/))), '"'),

    escape_sequence: (_) => token.immediate(/\\[nt"\\]/),

    // Fixed-point Q32.32, NOT a float — the toolchain has no floats anywhere.
    // The lexer picks the longest match, so `1.5` is a fixed_literal while
    // `1` is an integer_literal.
    fixed_literal: (_) => /\d+\.\d+/,

    integer_literal: (_) => /\d+/,

    boolean_literal: (_) => choice('true', 'false'),
  },
});

/** One or more `rule`, separated by commas. */
function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
