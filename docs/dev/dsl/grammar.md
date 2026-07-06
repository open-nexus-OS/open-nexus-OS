<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Grammar (v1 surface, normative intent)

This is the normative grammar for the `.nx` v1 surface. The compiler
(`userspace/dsl/core`) is the executable source of truth; divergences between this page
and the compiler are documentation bugs and must be fixed in the same change.

Notation: EBNF. `{ x }` = zero or more, `[ x ]` = optional, `|` = alternative,
terminals in quotes. Comments (`// …`) and whitespace are insignificant except inside
string literals.

## Compilation unit

```ebnf
File        = { Import } { Decl } ;
Import      = "import" StringLit ;                     (* explicit; no auto-import *)
Decl        = StoreDecl | EventDecl | ReduceDecl | EffectDecl
            | ComponentDecl | PageDecl | RoutesDecl ;
```

## State: stores, events, reducers, effects

The canonical shape (one file may hold all four; convention: `**.store.nx`):

```ebnf
StoreDecl   = "Store" Ident "{" { FieldDecl } "}" ;
FieldDecl   = Ident ":" Type [ "=" Expr ] [ "@persist" ] "," ;

EventDecl   = "Event" Ident "{" { EventCase } "}" ;
EventCase   = Ident [ "(" Type { "," Type } ")" ] "," ;

ReduceDecl  = "reduce" Ident "{" { ReduceArm } "}" ;     (* Ident = Event type *)
ReduceArm   = Pattern "=>" ( Stmt | Block ) "," ;
Pattern     = Ident [ "(" Ident { "," Ident } ")" ] ;

EffectDecl  = "@effect" "on" Ident [ "(" Ident { "," Ident } ")" ] Block ;
```

Reducer bodies are statements over `state` (the store the event's reducer is bound to):

```ebnf
Block       = "{" { Stmt } "}" ;
Stmt        = Assign | IfStmt | MatchStmt | LetStmt ;
Assign      = Place AssignOp Expr ";" ;
Place       = "state" "." Ident { "." Ident } ;
AssignOp    = "=" | "+=" | "-=" ;
LetStmt     = "let" Ident "=" Expr ";" ;
IfStmt      = "if" Expr Block [ "else" ( IfStmt | Block ) ] ;
MatchStmt   = "match" Expr "{" { Pattern "=>" ( Stmt | Block ) "," } "}" ;
```

Effect bodies additionally allow service calls and dispatch (and nothing else does):

```ebnf
EffectStmt  = LetStmt | SvcCall | Dispatch | MatchStmt | IfStmt ;
SvcCall     = "svc" "." Ident "." Ident "(" [ Args ] ")" ;
Dispatch    = "dispatch" "(" Ident [ "(" Args ")" ] ")" ";" ;
```

## Views: pages and components

A `Page` body **is** a view expression. A `Component` declares `props` first, then its
view.

```ebnf
PageDecl      = "Page" Ident "{" ViewNode "}" ;
ComponentDecl = "Component" Ident "{" [ PropsBlock ] ViewNode "}" ;
PropsBlock    = "props" ":" "{" { Ident ":" Type "," } "}" ;

ViewNode      = WidgetNode | IfView | ForView | CollectionView | MatchView ;
WidgetNode    = Ident ( PositionalSugar | PropBlock ) { Modifier } { Handler } ;
PositionalSugar = "(" Expr ")" ;                 (* Text("Hi") ≡ Text { value: "Hi" } *)
PropBlock     = "{" { PropInit | ViewNode } "}" ;
PropInit      = Ident ":" Expr [ ";" | "," ] ;

IfView        = "if" Expr "{" { ViewNode } "}"
                [ "else" ( IfView | "{" { ViewNode } "}" ) ] ;
ForView       = "for" Ident "in" Expr "{" { ViewNode } "}" ;      (* bounded *)
CollectionView= Ident "(" Expr ")" "{" Ident "in" { ViewNode } "}" ;
                (* e.g. List($state.users) { user in Text(user.name) } *)
MatchView     = "match" Expr "{" { Pattern "=>" "{" { ViewNode } "}" "," } "}" ;

Modifier      = "." Ident "(" [ Args ] ")" ;      (* catalog in modifiers.md *)
Handler       = "on" Ident "->" ( "emit" | "dispatch" ) "(" Ident [ "(" Args ")" ] ")" ;
```

## Routes

```ebnf
RoutesDecl  = "Routes" "{" { Route } "}" ;
Route       = StringLit "->" Ident [ "(" Ident ":" Type { "," Ident ":" Type } ")" ] ";" ;
```

## Expressions

```ebnf
Expr        = OrExpr ;
OrExpr      = AndExpr { "||" AndExpr } ;
AndExpr     = CmpExpr { "&&" CmpExpr } ;
CmpExpr     = AddExpr [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) AddExpr ] ;
AddExpr     = MulExpr { ( "+" | "-" ) MulExpr } ;
MulExpr     = UnaryExpr { ( "*" | "/" | "%" ) UnaryExpr } ;
UnaryExpr   = [ "!" | "-" ] Primary ;
Primary     = Literal | StateRef | PropsRef | DeviceRef | Ident
            | I18n | CallChain | "(" Expr ")" ;
StateRef    = "$state" "." Ident { "." Ident } ;
PropsRef    = "$props" "." Ident ;
DeviceRef   = "device" "." Ident { "." Ident } ;   (* read-only environment *)
I18n        = "@t" "(" StringLit { "," Expr } ")" ;
CallChain   = Ident { "." Ident "(" [ Args ] ")" } ; (* pure builders, e.g. QuerySpec *)
Args        = Expr { "," Expr } | Ident ":" Expr { "," Ident ":" Expr } ;
Literal     = IntLit | FxLit | StringLit | BoolLit | "[" [ Args ] "]" | EnumLit ;
EnumLit     = Ident "::" Ident ;
```

## Lexical

```ebnf
Ident       = letter { letter | digit | "_" } ;
IntLit      = digit { digit } ;
FxLit       = digit { digit } "." digit { digit } ;   (* fixed-point Fx, not float *)
StringLit   = '"' { char } '"' ;
BoolLit     = "true" | "false" ;
```

## Determinism rules bound to the grammar

- `parse → fmt → parse` is idempotent; the formatter defines the single canonical layout.
- `match` over an `Event` or `Enum` must be exhaustive (error otherwise).
- `for` iterates only over expressions with a statically known bound
  (a `List<T, cap>` or a literal range); anything else is an error.
- `if` on `device.profile` without a final `else` is a warning (`--deny-warn` promotes).
- Duplicate modifiers on one node are an error.
- List/collection templates require a stable key: `@key(expr)` modifier or the
  collection's documented key convention (error otherwise).

## Changelog

- **v1 (2026-07-06)** — canonical shape normalized: store fields declared directly
  (no `State {}` wrapper); `Event`/`reduce`/`@effect on` are top-level declarations;
  view conditionals are `if/else` (the former `@when/@else` form is removed);
  `Page` body is the view expression (no `view:` label); positional sugar for
  single-primary-prop widgets; collection templates `List(expr) { item in … }`.
