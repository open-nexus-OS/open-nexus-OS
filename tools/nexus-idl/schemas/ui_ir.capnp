@0xa7d3e9f1c2b45861;
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# Nexus UI DSL — canonical Scene IR (.nxir)
#
# The contract of the whole DSL system: the interpreter (app-host / in-compositor
# mount) and the AOT codegen both execute THIS structure under one written semantics.
# See docs/dev/dsl/ir.md (schema changelog lives there).
#
# DETERMINISM:
#   - all cross-references are u32 indices into canonically ordered lists
#   - symbols are interned, sorted, unique; everything names via symbol ids
#   - reducer/effect bodies are typed TOTAL expression trees (no back-edges,
#     termination by construction) — never bytecode
#   - same canonical source set => byte-identical .nxir (CI-proven)
#
# EVOLUTION:
#   - field numbers are append-only; minor bump = additive with defaults;
#     major bump = readers reject (docs/dev/dsl/ir.md#schema-evolution-rules)
#
# VERSION: 1.2 (TASK-0077B: Handler.bind)

struct UiProgram {
  schemaVersionMajor @0 :UInt16;   # readers reject unknown majors
  schemaVersionMinor @1 :UInt16;
  programHash        @2 :Data;     # SHA-256 over canonical bytes with this field zeroed
  sourceDigest       @3 :Data;     # SHA-256 of the canonical source set (provenance)
  symbols            @4 :List(Text); # interned, sorted, unique
  budgets            @5 :Budgets;
  types              @6 :List(TypeDef);     # program-declared enums/records
  stores             @7 :List(Store);
  events             @8 :List(EventDecl);
  reducers           @9 :List(Reducer);
  effects            @10 :List(EffectPlan);
  components         @11 :List(Component);  # pages are components with isPage
  routes             @12 :List(Route);
  i18nKeys           @13 :List(I18nKey);
  querySpecs         @14 :List(QuerySpec);  # v1 skeleton; grows in QuerySpec v1 task
  assets             @15 :List(AssetRef);
  entryPage          @16 :UInt32;           # index into components (must be a page)
}

struct Budgets {
  maxViewNodes   @0 :UInt32;
  maxExprNodes   @1 :UInt32;   # per reducer arm / effect step
  maxListLen     @2 :UInt32;   # default List<T> capacity
  maxStrLen      @3 :UInt32;   # default Str cap (bytes)
  maxEffectSteps @4 :UInt32;
  maxLocals      @5 :UInt32;   # per body
  maxChildren    @6 :UInt32;   # per view node
}

# ---------------------------------------------------------------- types

struct TypeRef {
  union {
    bool     @0 :Void;
    int      @1 :Void;          # i64; checked arithmetic
    fx       @2 :Void;          # Q32.32 fixed point (no floats anywhere)
    str      @3 :UInt32;        # length cap in bytes; 0 = budget default
    enumType @4 :UInt32;        # index into UiProgram.types (enumDef)
    record   @5 :UInt32;        # index into UiProgram.types (recordDef)
    list     @6 :ListType;
    option   @7 :TypeRef;
    result   @8 :ResultType;
    eventRef @9 :Void;          # a typed reference to an event case (props)
    idType   @10 :UInt32;       # nominal id type, symbol id (e.g. UserId)
    unit     @11 :Void;
    opaque   @12 :Void;         # not yet statically known (e.g. svc results
                                # before service signatures land); the loader
                                # skips re-typecheck for opaque-typed nodes
  }
}

struct ListType {
  elem @0 :TypeRef;
  cap  @1 :UInt32;              # 0 = budget default
}

struct ResultType {
  ok      @0 :TypeRef;
  errEnum @1 :UInt32;           # index into types; error-code enum
}

struct TypeDef {
  name @0 :UInt32;              # symbol id
  union {
    enumDef   @1 :EnumDef;
    recordDef @2 :RecordDef;
  }
}

struct EnumDef  { cases  @0 :List(EnumCase); }
struct EnumCase { name @0 :UInt32; payload @1 :List(TypeRef); }
struct RecordDef { fields @0 :List(FieldDef); }
struct FieldDef  { name @0 :UInt32; type @1 :TypeRef; }

# ---------------------------------------------------------------- state

struct Store {
  name   @0 :UInt32;            # symbol id
  fields @1 :List(StoreField);
}

struct StoreField {
  name    @0 :UInt32;
  type    @1 :TypeRef;
  default @2 :Expr;             # constant expression (validated const)
  persist @3 :Bool;             # @persist
}

struct EventDecl {
  name  @0 :UInt32;
  cases @1 :List(EnumCase);
}

struct Reducer {
  store @0 :UInt32;             # index into stores
  event @1 :UInt32;             # index into events
  arms  @2 :List(ReducerArm);   # exhaustive over event cases, case-index order
}

struct ReducerArm {
  case  @0 :UInt32;             # case index in the event decl
  binds @1 :List(UInt32);       # local slots receiving the case payload
  body  @2 :List(Stmt);
}

# ------------------------------------------------- statements & expressions
# Total by construction: no loops, no jumps. Iteration = capped combinators.

struct Stmt {
  union {
    set       @0 :SetStmt;
    letLocal  @1 :LetStmt;
    ifElse    @2 :IfStmt;
    matchEnum @3 :MatchStmt;
  }
}

enum AssignOp { assign @0; addAssign @1; subAssign @2; }

struct SetStmt {
  path  @0 :List(UInt32);       # state field path (field symbol ids)
  op    @1 :AssignOp;
  value @2 :Expr;
}

struct LetStmt { slot @0 :UInt32; value @1 :Expr; }

struct IfStmt {
  cond  @0 :Expr;
  then  @1 :List(Stmt);
  else  @2 :List(Stmt);
}

struct MatchStmt {
  scrutinee @0 :Expr;
  arms      @1 :List(StmtMatchArm);  # exhaustive, case-index order
}

struct StmtMatchArm {
  case  @0 :UInt32;
  binds @1 :List(UInt32);
  body  @2 :List(Stmt);
}

enum UnOpKind  { not @0; neg @1; }
enum BinOpKind {
  add @0; sub @1; mul @2; div @3; rem @4;
  eq @5; ne @6; lt @7; le @8; gt @9; ge @10;
  and @11; or @12; strConcat @13;
}

enum ListOpKind {
  len @0; get @1; append @2; removeWhere @3; map @4; filter @5; findFirst @6;
  isEmpty @7; contains @8;
}

struct Expr {
  type @0 :TypeRef;
  union {
    litBool    @1 :Bool;
    litInt     @2 :Int64;
    litFx      @3 :Int64;       # raw Q32.32
    litStr     @4 :Text;
    litEnum    @5 :EnumLit;
    litList    @6 :List(Expr);
    fieldGet   @7 :FieldGet;    # store state read
    localGet   @8 :UInt32;      # let-slot / bind-slot
    paramGet   @9 :UInt32;      # component prop / route param slot
    unOp       @10 :UnExpr;
    binOp      @11 :BinExpr;
    listOp     @12 :ListOpExpr;
    recordGet  @13 :RecordGet;
    recordMake @14 :List(Expr); # field values in RecordDef field order
    fmtI18n    @15 :I18nExpr;
    deviceGet  @16 :UInt32;     # device environment field id (stable table)
    optionSome @17 :Expr;
    optionNone @18 :Void;
  }
}

struct EnumLit  { enumType @0 :UInt32; case @1 :UInt32; payload @2 :List(Expr); }
struct FieldGet { store @0 :UInt32; path @1 :List(UInt32); }
struct UnExpr   { op @0 :UnOpKind;  operand @1 :Expr; }
struct BinExpr  { op @0 :BinOpKind; lhs @1 :Expr; rhs @2 :Expr; }
struct RecordGet { base @0 :Expr; field @1 :UInt32; }

struct ListOpExpr {
  op      @0 :ListOpKind;
  base    @1 :Expr;
  arg     @2 :Expr;             # element / index / predicate input (op-specific)
  lambda  @3 :LambdaExpr;       # for map/filter/findFirst/removeWhere
}

struct LambdaExpr { bindSlot @0 :UInt32; body @1 :Expr; }

struct I18nExpr { key @0 :UInt32; args @1 :List(Expr); }  # key = i18nKeys index

# ---------------------------------------------------------------- effects
# Bounded plans, not code. IO exists only here.

struct EffectPlan {
  event @0 :UInt32;             # index into events
  case  @1 :UInt32;             # trigger case
  binds @2 :List(UInt32);       # payload binds
  steps @3 :List(EffectStep);
}

struct EffectStep {
  union {
    call     @0 :CallStep;
    dispatch @1 :DispatchStep;
    query    @2 :QueryStep;
  }
}

struct CallStep {
  service    @0 :UInt32;        # symbol id (svc.<service>)
  method     @1 :UInt32;        # symbol id
  args       @2 :List(Expr);
  timeoutMs  @3 :UInt32;        # mandatory, > 0
  resultSlot @4 :UInt32;        # local slot receiving Result<T, E>
  onOk       @5 :DispatchStep;
  onErr      @6 :DispatchStep;
}

struct DispatchStep {
  event   @0 :UInt32;
  case    @1 :UInt32;
  payload @2 :List(Expr);
}

struct QueryStep {
  spec     @0 :UInt32;          # index into querySpecs
  args     @1 :List(Expr);      # query param values, declaration order
  onPage   @2 :DispatchStep;
  onErr    @3 :DispatchStep;
  # --- v1.3 (QuerySpec v1) ---
  token    @4 :Expr;            # page token (Str; "" = first page)
  rowsSlot @5 :UInt32;          # local receiving the page rows (Ok path)
                                # (the Err path binds the error code here —
                                #  only one path ever runs)
  nextSlot @6 :UInt32;          # local receiving the next-page token (Str)
}

# ---------------------------------------------------------------- views

struct Component {
  name  @0 :UInt32;
  isPage @1 :Bool;
  props @2 :List(FieldDef);
  view  @3 :ViewNode;
}

struct ViewNode {
  nodeId @0 :UInt64;            # persisted stable identity (docs/dev/dsl/ir.md)
  union {
    widget       @1 :Widget;
    forEach      @2 :ForEach;
    branch       @3 :Branch;
    componentRef @4 :ComponentRef;
  }
}

struct Widget {
  kind      @0 :UInt32;         # widget symbol id (registry-validated)
  props     @1 :List(PropInit);
  modifiers @2 :List(Modifier); # canonical catalog order; duplicates rejected
  handlers  @3 :List(Handler);
  children  @4 :List(ViewNode);
}

struct PropInit { name @0 :UInt32; value @1 :Expr; }

struct Modifier {
  modId @0 :UInt16;             # index into the generated modifier table
  args  @1 :List(TokenArg);
}

struct TokenArg {
  union {
    token   @0 :UInt32;         # semantic token symbol id
    int     @1 :Int64;
    fx      @2 :Int64;
    boolean @3 :Bool;
    expr    @4 :Expr;           # e.g. .key(expr), .animate(value:)
  }
}

struct Handler {
  trigger @0 :UInt32;           # interaction symbol id (Tap, Change, Submit, ...)
  union {
    dispatch @1 :DispatchStep;  # dispatch a store event
    emitProp @2 :EmitProp;      # emit an EventRef prop (components)
    navigate @3 :Expr;          # v1.1: route path expression (Str-typed)
    bind @4 :FieldGet;          # v1.2: two-way binding write target — the
                                # interaction value writes this state path
                                # (auto-synthesized for `checked:/value:`
                                # props bound to $state on interactive kinds)
  }
}

struct EmitProp { prop @0 :UInt32; payload @1 :List(Expr); }

struct ForEach {
  binding  @0 :Expr;            # List<T> expression
  bindSlot @1 :UInt32;          # the `item in` slot
  keyExpr  @2 :Expr;            # stable key (required for collections)
  template @3 :ViewNode;
  windowed @4 :Bool;            # collection template (List(...)) vs static for
}

struct Branch {
  arms     @0 :List(BranchArm); # evaluated in order, first match wins
  elseBody @1 :List(ViewNode);  # may be empty (lint warns on device.profile)
}

struct BranchArm { cond @0 :Expr; body @1 :List(ViewNode); }

struct ComponentRef {
  component @0 :UInt32;         # index into components
  args      @1 :List(PropInit);
}

# ------------------------------------------------------------- navigation

struct Route {
  path   @0 :Text;              # "/detail/:id"
  page   @1 :UInt32;            # index into components (isPage)
  params @2 :List(FieldDef);    # typed route params
}

# ------------------------------------------------------------------ i18n

struct I18nKey {
  key      @0 :UInt32;          # symbol id of the dotted key
  argTypes @1 :List(TypeRef);
}

# ------------------------------------------------------------ query specs
# v1.3 (QuerySpec v1, docs/dev/dsl/db-queries.md): typed bounded query values.
# Built purely (a top-level `Query` declaration), executed ONLY via the
# `query` effect step. Predicate values are const literals or paramGet exprs
# (bound from QueryStep.args at execution).

struct QuerySpec {
  source @0 :UInt32;            # table/source symbol id
  # --- v1.3 ---
  paramCount @1 :UInt16;        # declared params (call-site arity check)
  preds      @2 :List(QueryPred);
  orderCol   @3 :UInt32;        # column symbol id (the scan index)
  descending @4 :Bool;
  limit      @5 :UInt32;        # mandatory, > 0
}

struct QueryPred {
  col   @0 :UInt32;             # column symbol id
  op    @1 :QueryOp;
  value @2 :Expr;               # const literal or paramGet only
}

# v1 floor: eq + inclusive bounds; strict `<`/`>` land with the v2 builder
# (lowering rejects them with a stable diagnostic today).
enum QueryOp { eq @0; ge @1; le @2; }

# ---------------------------------------------------------------- assets

enum AssetKind { svg @0; localeCatalog @1; raw @2; }

struct AssetRef {
  name   @0 :UInt32;
  kind   @1 :AssetKind;
  digest @2 :Data;              # SHA-256 of the asset bytes
}
