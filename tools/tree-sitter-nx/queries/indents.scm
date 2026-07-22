; Indentation.
;
; NOTE ON AUTHORITY: `nx-dsl fmt` defines the one canonical layout for .nx
; (`parse -> fmt -> parse` is idempotent). These captures only make typing
; feel right; they are not a second opinion on formatting. When in doubt,
; run the formatter (<leader>f in Neovim).
;
; Consumers: Helix and nvim-treesitter's indent module. Stock Neovim has no
; tree-sitter indent engine, so ftplugin/nx.lua relies on autoindent plus
; these braces — see the README.

[
  (block)
  (view_block)
  (widget_body)
  (props_block)
  (state_block)
  (store_declaration)
  (event_declaration)
  (reduce_declaration)
  (component_declaration)
  (page_declaration)
  (routes_declaration)
  (window_declaration)
  (query_declaration)
  (match_view)
  (match_statement)
  (list_literal)
  (arguments)
] @indent.begin

[
  "}"
  ")"
  "]"
] @indent.branch

[
  "}"
  ")"
  "]"
] @indent.end

(comment) @indent.ignore
