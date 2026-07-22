; Highlighting for the .nx DSL.
;
; Capture names follow the Neovim standard groups (`:help treesitter-highlight-groups`)
; and are chosen to match the classes the VS Code extension already assigns in
; nx-dsl-vscode/syntaxes/nx.tmLanguage.json, so both editors agree:
;
;   keyword.other.declaration.nx    -> @keyword
;   variable.language.state.nx      -> @variable.builtin
;   keyword.other.effect.nx         -> @attribute
;   support.function.i18n.nx        -> @function.builtin
;   entity.name.function.modifier   -> @function.method.call
;   entity.name.type.nx             -> @type
;   constant.numeric.fixed.nx       -> @number.float
;
; Later patterns win, so generic rules come first and specific ones last.

; ---------------------------------------------------------------- literals

(comment) @comment @spell

(string_literal) @string
(escape_sequence) @string.escape

(integer_literal) @number
; Q32.32 fixed-point, not a float — but @number.float is the group editors
; colour distinctly, which is exactly the visual cue we want.
(fixed_literal) @number.float
(boolean_literal) @boolean

; ---------------------------------------------------------------- keywords

[
  "Store"
  "Event"
  "reduce"
  "Page"
  "Component"
  "Routes"
  "Window"
  "Query"
  "props"
] @keyword

"import" @keyword.import

[
  "if"
  "else"
  "match"
] @keyword.conditional

[
  "for"
  "in"
] @keyword.repeat

"let" @keyword

; Query clause keywords — contextual, only keywords inside a Query body.
[
  "params"
  "where"
  "orderBy"
  "limit"
  "asc"
  "desc"
] @keyword

; Handler / effect actions.
[
  "dispatch"
  "emit"
  "navigate"
  "on"
] @keyword.function

; ------------------------------------------------------------------ sigils

; `@effect` and `@persist` are attributes on a declaration/field.
"@effect" @attribute
(persist_attribute) @attribute

; `@t("key")` is the i18n builtin.
"@t" @function.builtin
(i18n_call key: (string_literal) @string.special)

; Ambient roots: none of these are user-defined names.
[
  "$state"
  "$props"
  "state"
  "device"
  "svc"
] @variable.builtin

; ------------------------------------------------------------------- types

; Every position that names a type, widget, component, page or case.
(type_identifier) @type

; Declaration sites read as definitions.
(store_declaration name: (type_identifier) @type.definition)
(event_declaration name: (type_identifier) @type.definition)
(component_declaration name: (type_identifier) @type.definition)
(page_declaration name: (type_identifier) @type.definition)
(query_declaration name: (type_identifier) @type.definition)

; Event cases and patterns are constructors, not types.
(event_case name: (type_identifier) @constructor)
(pattern case: (type_identifier) @constructor)
(enum_literal case: (type_identifier) @constructor)
(dispatch_action case: (type_identifier) @constructor)
(dispatch_statement case: (type_identifier) @constructor)

; Interaction triggers (Tap, Change, Submit, ...) are a closed vocabulary.
(handler trigger: (type_identifier) @constant.builtin)

; --------------------------------------------------------------- functions

; `.padding(4)` — the modifier catalog (docs/dev/dsl/modifiers.md).
(modifier name: (identifier) @function.method.call)

(call_expression function: (path_expression) @function.call)
(service_call segment: (identifier) @function.call)

; ------------------------------------------------------------- identifiers

; Field/property names on the left of a colon.
(property name: (identifier) @property)
(store_field name: (identifier) @property)
(state_field name: (identifier) @property)
(prop_declaration name: (identifier) @property)
(window_field name: (identifier) @property)
(route_parameter name: (identifier) @property)
(named_argument name: (identifier) @variable.parameter)
(where_clause column: (identifier) @property)
(order_by_clause column: (identifier) @property)

; Member access: the part after a dot.
(path_expression field: (identifier) @variable.member)
(state_reference field: (identifier) @variable.member)
(props_reference field: (identifier) @variable.member)
(device_reference field: (identifier) @variable.member)
(state_place field: (identifier) @variable.member)

; Bindings introduced by the language.
(item_binding name: (identifier) @variable.parameter)
(for_view binding: (identifier) @variable.parameter)
(pattern binding: (identifier) @variable.parameter)
(let_statement name: (identifier) @variable)

; Route paths are more than plain strings.
(route path: (string_literal) @string.special.path)
(import path: (string_literal) @string.special.path)

; --------------------------------------------------------------- operators

[
  "="
  "+="
  "-="
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "+"
  "-"
  "*"
  "/"
  "%"
  "!"
  "&&"
  "||"
  "->"
  "=>"
  "::"
] @operator

[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
] @punctuation.bracket

[
  ","
  ";"
  ":"
  "."
] @punctuation.delimiter
