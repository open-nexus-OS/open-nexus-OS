<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Grammar (v1 surface, normative intent)

This is the normative grammar for the `.nx` v1 surface. The compiler
(`userspace/dsl/core`) is the executable source of truth; divergences between this page
and the compiler are documentation bugs and must be fixed in the same change.

Notation: EBNF. `{ x }` = zero or more, `[ x ]` = optional, `|` = alternative,
terminals in quotes. Comments (`// …`) and whitespace are insignificant except inside
string literals.

This page describes what the **parser** accepts. A few rules that read like syntax are
enforced later, by the checker or by lowering; those are called out where they apply and
collected under [Determinism rules](#determinism-rules-bound-to-the-grammar).

## Compilation unit

```ebnf
File        = { Import } { Decl } ;
Import      = "import" StringLit ;                     (* explicit; no auto-import *)
Decl        = StoreDecl | EventDecl | ReduceDecl | EffectDecl
            | ComponentDecl | PageDecl | RoutesDecl | WindowDecl | QueryDecl ;
```

Imports must precede declarations.

## State: stores, events, reducers, effects

The canonical shape (one file may hold all four; convention: `**.store.nx`):

```ebnf
StoreDecl   = "Store" Ident "{" { FieldDecl } "}" ;
FieldDecl   = Ident ":" Type [ "=" Expr ] [ "@persist" ] "," ;   (* @persist follows the default *)

EventDecl   = "Event" Ident "{" { EventCase } "}" ;
EventCase   = Ident [ "(" Type { "," Type } ")" ] "," ;

ReduceDecl  = "reduce" Ident "{" ReduceArm { ReduceArm } "}" ;   (* Ident = Event type; ≥1 arm *)
ReduceArm   = Pattern "=>" ( ArmStmt | Block ) "," ;
Pattern     = Ident [ "(" Ident { "," Ident } ")" ] ;

EffectDecl  = "@effect" "on" Pattern Block ;
```

## Windows

App-owned window intent (`docs/dev/ui/patterns/windowing/window-intent.md`). Fields are
optional and order-free; omitted fields take the defaults `titlebar` / `auto` / `normal` /
`false`. At most one `Window` per program (enforced in lowering, not here).

```ebnf
WindowDecl  = "Window" "{" { WindowField } "}" ;
WindowField = ( "style"     ":" ( "titlebar" | "hiddenTitlebar" | "plain" )
              | "mode"      ":" ( "auto" | "freeform" | "fullscreen" )
              | "level"     ":" ( "normal" | "desktop" | "overlay" )
              | "resizable" ":" BoolLit
              ) [ "," | ";" ] ;                       (* separator optional on the last field *)
```

## Queries (QuerySpec v1, docs/dev/dsl/db-queries.md)

`Query`, `params`, `where`, `orderBy`, `limit`, `asc`, `desc` are contextual
(recognized only in these positions — they stay usable as ordinary names):

```ebnf
QueryDecl   = "Query" Ident "on" Ident "{" { QueryClause } "}" ;
QueryClause = ParamsClause | WhereClause | OrderClause | LimitClause ;
ParamsClause = "params" ":" "{" { Ident ":" Type "," } "}" "," ;
WhereClause  = "where" Ident ( "==" | ">=" | "<=" ) Expr "," ;
OrderClause  = "orderBy" Ident [ "asc" | "desc" ] "," ;      (* mandatory, once *)
LimitClause  = "limit" IntLit "," ;                          (* mandatory, once *)
```

The parser also *accepts* `>` and `<` in a `WhereClause`, but the checker rejects them
(`NX0410`): strict bounds are reserved for the v2 builder. They are not part of the v1
surface, so they are absent above.

Execution is an effect statement: `match QueryName(NamedArgs) { Ok(rows,
next) => Dispatch , Err(e) => Dispatch , }` — the only query execution site.

## Statements

Statement bodies appear in effects (`EffectDecl`) and reducer arms (`ReduceArm`).

A `;` terminates a simple statement **inside a block**. A single-statement arm omits it —
the arm's `,` already terminates it (`Inc => state.value += 1,`).

```ebnf
Block       = "{" { BlockStmt } "}" ;
BlockStmt   = SimpleStmt ";" | IfStmt | MatchStmt ;
ArmStmt     = SimpleStmt | IfStmt | MatchStmt ;     (* single-statement arm: no ";" *)

SimpleStmt  = Assign | LetStmt | Dispatch | SvcCall ;
Assign      = Place AssignOp Expr ;
Place       = "state" "." Ident { "." Ident } ;
AssignOp    = "=" | "+=" | "-=" ;
LetStmt     = "let" Ident "=" Expr ;
Dispatch    = "dispatch" "(" Ident [ "(" Args ")" ] ")" ;
SvcCall     = "svc" "." Ident { "." Ident } "(" [ Args ] ")" ;

IfStmt      = "if" Expr Block [ "else" ( IfStmt | Block ) ] ;
MatchStmt   = "match" Expr "{" MatchArm { MatchArm } "}" ;   (* ≥1 arm *)
MatchArm    = Pattern "=>" ( ArmStmt | Block ) "," ;
```

The **parser accepts this union everywhere**; the *checker* decides which statement kinds
are legal where, so it can report a precise span. Reducers are pure: `SvcCall` and
`Dispatch` in a reducer body are checker errors, not parse errors. Effects own all IO.

## Views: pages and components

A `Page` body **is** a view expression. A `Component` declares `props` first, then
optionally component-local `state`, then its view.

```ebnf
PageDecl      = "Page" Ident "{" ViewNode "}" ;
ComponentDecl = "Component" Ident "{" [ PropsBlock ] [ StateBlock ] ViewNode "}" ;
PropsBlock    = "props" ":" "{" { Ident ":" Type "," } "}" ;
StateBlock    = "state" ":" "{" { Ident ":" Type [ "=" Expr ] "," } "}" ;

ViewNode      = WidgetNode | IfView | ForView | CollectionView | MatchView ;
WidgetNode    = Ident [ PositionalSugar ] [ PropBlock ] { Modifier } { Handler } ;
PositionalSugar = "(" Expr ")" ;                 (* Text("Hi") ≡ Text { value: "Hi" } *)
PropBlock     = "{" { PropInit | Handler [ ";" | "," ] | ViewNode } "}" ;
PropInit      = Ident ":" Expr [ ";" | "," ] ;

IfView        = "if" Expr ViewBlock { "else" "if" Expr ViewBlock } [ "else" ViewBlock ] ;
ForView       = "for" Ident "in" Expr ViewBlock ;                (* bounded *)
CollectionView= Ident "(" Expr ")" "{" Ident "in" { ViewNode } "}" { Modifier } ;
                (* e.g. List($state.users) { user in Text(user.name) } *)
MatchView     = "match" Expr "{" ViewArm { ViewArm } "}" ;       (* ≥1 arm *)
ViewArm       = Pattern "=>" ViewBlock "," ;
ViewBlock     = "{" { ViewNode } "}" ;

Modifier      = "." Ident "(" [ Args ] ")" ;      (* catalog in modifiers.md *)
Handler       = "on" Ident "->" HandlerAction ;
HandlerAction = "dispatch" "(" Ident [ "(" Args ")" ] ")"
              | "emit" "(" Expr { "," Expr } ")"
              | "navigate" "(" Expr ")" ;
```

Both `PositionalSugar` and `PropBlock` are optional and may co-occur: `Spacer` is a
complete node, and `List($state.rows) { row in … }` uses both.

**Handlers bind greedily to the nearest preceding widget.** A handler written after a
node's modifiers belongs to that node, not to the enclosing `PropBlock`:

```nx
Stack {
    Button { label: "-" }.bg(surfaceVariant)
    on Tap -> dispatch(Dec)        // Button's handler, not Stack's
}
```

A handler *inside* a `PropBlock` that follows no widget — first entry, or right after a
`PropInit` — belongs to the enclosing node and may take a `,` or `;` separator.

## Routes

```ebnf
RoutesDecl  = "Routes" "{" { Route } "}" ;
Route       = StringLit "->" Ident [ "(" Ident ":" Type { "," Ident ":" Type } ")" ] ";" ;
```

A route path must start with `/`.

## Expressions

```ebnf
Expr        = OrExpr ;
OrExpr      = AndExpr { "||" AndExpr } ;
AndExpr     = CmpExpr { "&&" CmpExpr } ;
CmpExpr     = AddExpr [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) AddExpr ] ;  (* non-associative *)
AddExpr     = MulExpr { ( "+" | "-" ) MulExpr } ;
MulExpr     = UnaryExpr { ( "*" | "/" | "%" ) UnaryExpr } ;
UnaryExpr   = [ "!" | "-" ] Primary ;
Primary     = Literal | StateRef | PropsRef | DeviceRef | SvcCall
            | I18n | EnumLit | Call | Path | "(" Expr ")" ;

StateRef    = ( "$state" | "state" ) "." Ident { "." Ident } ;  (* bare `state` = reducer/effect read *)
PropsRef    = "$props" "." Ident { "." Ident } ;
DeviceRef   = "device" "." Ident { "." Ident } ;   (* read-only environment *)
I18n        = "@t" "(" StringLit { "," Expr } ")" ;
Path        = Ident { "." Ident } ;                (* plain field access, e.g. `user.name` *)
Call        = Path "(" [ Args ] ")" ;              (* pure builders, e.g. QuerySpec *)
Args        = Arg { "," Arg } ;
Arg         = [ Ident ":" ] Expr ;                 (* positional and named may be mixed *)
Literal     = IntLit | FxLit | StringLit | BoolLit | ListLit ;
ListLit     = "[" [ Expr { "," Expr } ] "]" ;
EnumLit     = Ident "::" Ident [ "(" [ Expr { "," Expr } ] ")" ] ;
```

`$state`/`$props` and `@t`/`@effect`/`@persist` are the only sigils; any other `$name` or
`@name` is a lexical error.

## Lexical

```ebnf
Ident       = ( letter | "_" ) { letter | digit | "_" } ;
Type        = Ident [ "<" Type { "," Type } ">" ] ;   (* e.g. List<Msg>, List<List<Int>> *)
IntLit      = digit { digit } ;
FxLit       = digit { digit } "." digit { digit } ;   (* fixed-point Fx, not float *)
StringLit   = '"' { char | Escape } '"' ;             (* char excludes '"', '\', newline *)
Escape      = "\" ( "n" | "t" | '"' | "\" ) ;         (* closed set; anything else is an error *)
BoolLit     = "true" | "false" ;
Comment     = "//" { char } ;                         (* to end of line; no block form *)
```

Bounds the lexer/parser enforce (hard determinism requirements, not style):
file ≤ 512 KiB, identifier ≤ 128 bytes, structural nesting ≤ 64 levels,
`IntLit` fits `i64`, `FxLit` fits Q32.32.

## Determinism rules bound to the grammar

- `parse → fmt → parse` is idempotent; the formatter defines the single canonical layout.
- `match` over an `Event` or `Enum` must be exhaustive (error otherwise).
- `for` iterates only over expressions with a statically known bound
  (a `List<T, cap>` or a literal range); anything else is an error.
- `if` on `device.profile` without a final `else` is a warning (`--deny-warn` promotes).
- Duplicate modifiers on one node are an error; so is setting the same prop twice.
- List/collection templates require a stable key: `@key(expr)` modifier or the
  collection's documented key convention (error otherwise).
- Reducers are pure — no `svc.*`, no `dispatch`, no time or randomness.
- `Query` needs both `orderBy` and `limit`; each may appear only once.
- At most one `Window` declaration per program.

## Changelog

- **v1.1 (2026-07-22)** — reconciled with the compiler after
  `tools/tree-sitter-nx` was derived from it. All changes document behaviour that
  already existed; no language change. `WindowDecl` added (was entirely absent);
  `ComponentDecl` gained `[ StateBlock ]`; `HandlerAction` split out and extended with
  `navigate`, with `emit` taking an `Expr` rather than an `Ident`; `PropBlock` may hold
  handlers; `WidgetNode` makes both the positional sugar and the prop block optional;
  `CollectionView` may carry trailing modifiers; `WhereClause` gained a note that the
  parser tolerates `>`/`<` while the checker rejects them (`NX0410`, reserved for v2);
  statement `;` attributed to the in-block form only (`ArmStmt` vs `BlockStmt`);
  `PropsRef` allows multi-segment paths; `CallChain` split into `Path` and `Call` so a
  plain `user.name` is expressible; `EnumLit` may carry arguments; `Args` may mix
  positional and named; `ListLit` separated from `Args`; `Ident` may start with `_`;
  `Type`, `Escape` and `Comment` productions added (previously used but undefined);
  `reduce`/`match` documented as needing ≥1 arm; lexer bounds recorded.
- **v1 (2026-07-06)** — canonical shape normalized: store fields declared directly
  (no `State {}` wrapper); `Event`/`reduce`/`@effect on` are top-level declarations;
  view conditionals are `if/else` (the former `@when/@else` form is removed);
  `Page` body is the view expression (no `view:` label); positional sugar for
  single-primary-prop widgets; collection templates `List(expr) { item in … }`.
