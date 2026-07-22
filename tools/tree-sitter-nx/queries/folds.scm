; Foldable regions.
;
; Neovim uses these via `foldexpr=v:lua.vim.treesitter.foldexpr()`
; (set in ~/.config/nvim/lua/config/options.lua).
;
; Every declaration folds, plus every brace body inside one — so a long page
; collapses to its `Page Name {` line, and inside it each widget subtree
; collapses on its own.

[
  (store_declaration)
  (event_declaration)
  (reduce_declaration)
  (effect_declaration)
  (component_declaration)
  (page_declaration)
  (routes_declaration)
  (window_declaration)
  (query_declaration)
] @fold

[
  (block)
  (view_block)
  (widget_body)
  (props_block)
  (state_block)
  (params_clause)
] @fold

[
  (match_view)
  (match_statement)
  (if_view)
  (if_statement)
  (for_view)
] @fold
