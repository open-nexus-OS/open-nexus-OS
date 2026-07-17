// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! AST for the `.nx` v1 surface (docs/dev/dsl/grammar.md).
//!
//! Spans everywhere; owned strings (interning happens at lowering). The AST is
//! shaped for three consumers: the formatter (canonical print), the checker
//! (resolve/typeck/lints), and the lowering pass (AST → IR).

use crate::diag::Span;
use alloc::{boxed::Box, string::String, vec::Vec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub text: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub imports: Vec<Import>,
    pub decls: Vec<Decl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    Store(StoreDecl),
    Event(EventDecl),
    Reduce(ReduceDecl),
    Effect(EffectDecl),
    Component(ComponentDecl),
    Page(PageDecl),
    Routes(RoutesDecl),
    Query(QueryDecl),
    Window(WindowDecl),
}

// --------------------------------------------------------------- window intent

/// App-owned window intent (`Window { style: …, mode: …, level: …, resizable: … }`).
/// At most one per program; absent = the defaults below. The compositor composes
/// the frame under the active windowing policy — see
/// `docs/dev/ui/patterns/windowing/window-intent.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDecl {
    pub style: WindowStyle,
    pub mode: WindowMode,
    pub level: WindowLevel,
    pub resizable: bool,
    pub span: Span,
}

/// How much frame the app *wants* (the policy may clamp it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowStyle {
    #[default]
    Titlebar,
    HiddenTitlebar,
    Plain,
}

/// Preferred default window mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMode {
    #[default]
    Auto,
    Freeform,
    Fullscreen,
}

/// Z-band. `desktop` is capability-gated (only the product's selected shell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowLevel {
    #[default]
    Normal,
    Desktop,
    Overlay,
}

// ------------------------------------------------------------------- state

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDecl {
    pub name: Ident,
    pub fields: Vec<StoreField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreField {
    pub name: Ident,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub persist: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDecl {
    pub name: Ident,
    pub cases: Vec<EventCase>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCase {
    pub name: Ident,
    pub payload: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReduceDecl {
    pub event: Ident,
    pub arms: Vec<ReduceArm>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReduceArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// `CaseName` or `CaseName(bind, ...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub case: Ident,
    pub binds: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDecl {
    pub trigger: Pattern,
    pub body: Vec<Stmt>,
    pub span: Span,
}

// ------------------------------------------------------------------ queries

/// `Query Name on source { params: {…}, where col op value, orderBy col
/// [desc], limit N, }` — an immutable QuerySpec value (docs/dev/dsl/
/// db-queries.md). Built purely at the top level; executed ONLY via a
/// `match Name(args, token: t) { Ok(rows, next) => …, Err(e) => … }` step
/// inside an `@effect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDecl {
    pub name: Ident,
    /// Table/source name (validated at the service boundary).
    pub source: Ident,
    pub params: Vec<PropDecl>,
    pub preds: Vec<QueryPred>,
    /// The order column — also the only column ranges may target (v1 rule).
    pub order_col: Ident,
    pub descending: bool,
    pub limit: i64,
    pub limit_span: Span,
    pub span: Span,
}

/// One `where col <op> value` clause. `value` is a const literal or a
/// param reference (checked); `op` is `==`, `>=`, or `<=` in v1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPred {
    pub col: Ident,
    pub op: BinOp,
    pub value: Expr,
    pub span: Span,
}

// ------------------------------------------------------------------- types

/// `Bool`, `List<User>`, `Result<T, E>` — name + optional angle args.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeExpr {
    pub name: Ident,
    pub args: Vec<TypeExpr>,
    pub span: Span,
}

// -------------------------------------------------------------- statements

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// `state.field.path <op> expr;`
    Assign { path: Vec<Ident>, op: AssignOp, value: Expr, span: Span },
    /// `let name = expr;`
    Let { name: Ident, value: Expr, span: Span },
    /// `if cond { .. } else { .. }`
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt>, span: Span },
    /// `match expr { Pattern => { .. }, }`
    Match { scrutinee: Expr, arms: Vec<StmtMatchArm>, span: Span },
    /// `dispatch(Case(args));` — effects only (checked later, parsed anywhere).
    Dispatch { case: Ident, args: Vec<Expr>, span: Span },
    /// Bare call statement (`svc.log.write(msg);`) — effects only.
    ExprStmt { expr: Expr, span: Span },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StmtMatchArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
    pub span: Span,
}

// ------------------------------------------------------------- expressions

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Bool {
        value: bool,
        span: Span,
    },
    Int {
        value: i64,
        span: Span,
    },
    /// Raw Q32.32.
    Fx {
        value: i64,
        span: Span,
    },
    Str {
        value: String,
        span: Span,
    },
    /// `[a, b, c]`
    List {
        items: Vec<Expr>,
        span: Span,
    },
    /// `Type::Case` or `Type::Case(args)`
    EnumLit {
        ty: Ident,
        case: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    /// `$state.a.b`
    StateRef {
        path: Vec<Ident>,
        span: Span,
    },
    /// `$props.a`
    PropsRef {
        path: Vec<Ident>,
        span: Span,
    },
    /// `device.profile`
    DeviceRef {
        path: Vec<Ident>,
        span: Span,
    },
    /// `user.name` — a local/bind followed by field accesses.
    Path {
        segments: Vec<Ident>,
        span: Span,
    },
    /// `svc.users.list(args)` / builder chains `q.limit(5)` — a path call.
    Call {
        path: Vec<Ident>,
        args: Vec<CallArg>,
        span: Span,
    },
    /// `@t("key", args)`
    I18n {
        key: String,
        key_span: Span,
        args: Vec<Expr>,
        span: Span,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Expr::Bool { span, .. }
            | Expr::Int { span, .. }
            | Expr::Fx { span, .. }
            | Expr::Str { span, .. }
            | Expr::List { span, .. }
            | Expr::EnumLit { span, .. }
            | Expr::StateRef { span, .. }
            | Expr::PropsRef { span, .. }
            | Expr::DeviceRef { span, .. }
            | Expr::Path { span, .. }
            | Expr::Call { span, .. }
            | Expr::I18n { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. } => *span,
        }
    }
}

/// `expr` or `name: expr` (named args, e.g. `timeoutMs = 250` uses `=`? No —
/// named call args use `name: expr`; `timeoutMs = 250` sugar is rejected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArg {
    pub name: Option<Ident>,
    pub value: Expr,
}

// ------------------------------------------------------------------- views

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDecl {
    pub name: Ident,
    pub view: ViewNode,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDecl {
    pub name: Ident,
    pub props: Vec<PropDecl>,
    /// Local `$state` fields (`state: { open: Bool = false, }`) — lowered to
    /// an implicit store owned by this component.
    pub state: Vec<StoreField>,
    pub view: ViewNode,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropDecl {
    pub name: Ident,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewNode {
    /// `Name { props/children }` / `Name(positional)` + modifiers + handlers.
    /// Widget vs component reference is decided at resolve time.
    Widget(WidgetNode),
    /// `if cond { .. } else if .. else { .. }` — flattened arms.
    If { arms: Vec<(Expr, Vec<ViewNode>)>, els: Vec<ViewNode>, span: Span },
    /// `for x in xs { .. }` (static bounded)
    For { var: Ident, iter: Expr, body: Vec<ViewNode>, span: Span },
    /// `List($state.users) { user in .. }` (keyed, virtualizable)
    Collection(CollectionNode),
    /// `match expr { Case => { .. }, }`
    Match { scrutinee: Expr, arms: Vec<ViewMatchArm>, span: Span },
}

impl ViewNode {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            ViewNode::Widget(w) => w.span,
            ViewNode::If { span, .. }
            | ViewNode::For { span, .. }
            | ViewNode::Match { span, .. } => *span,
            ViewNode::Collection(c) => c.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetNode {
    pub name: Ident,
    /// `Text("hi")` positional sugar for the primary prop.
    pub positional: Option<Expr>,
    pub props: Vec<(Ident, Expr)>,
    pub children: Vec<ViewNode>,
    pub modifiers: Vec<ModifierCall>,
    pub handlers: Vec<HandlerDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionNode {
    /// The collection widget (e.g. `List`).
    pub kind: Ident,
    pub binding: Expr,
    pub var: Ident,
    pub body: Vec<ViewNode>,
    pub modifiers: Vec<ModifierCall>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewMatchArm {
    pub pattern: Pattern,
    pub body: Vec<ViewNode>,
    pub span: Span,
}

/// `.name(args)` — catalog-validated at resolve time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifierCall {
    pub name: Ident,
    pub args: Vec<CallArg>,
    pub span: Span,
}

/// `on Tap -> dispatch(Case(args))` / `on Tap -> emit($props.onOpen)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerDecl {
    pub trigger: Ident,
    pub action: HandlerAction,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerAction {
    Dispatch {
        case: Ident,
        args: Vec<Expr>,
    },
    Emit {
        prop: Expr,
        args: Vec<Expr>,
    },
    /// `navigate("/detail/7")` — a Str-typed route-path expression.
    Navigate {
        path: Expr,
    },
}

// ---------------------------------------------------------------- routing

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutesDecl {
    pub routes: Vec<Route>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub path: String,
    pub path_span: Span,
    pub page: Ident,
    pub params: Vec<(Ident, TypeExpr)>,
    pub span: Span,
}
