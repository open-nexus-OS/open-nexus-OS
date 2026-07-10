// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Lowering: checked AST → canonical `.nxir` bytes.
//!
//! Canonicalization rules (docs/dev/dsl/ir.md): interned sorted symbols,
//! components/stores/events sorted by name (source order never leaks into the
//! IR), reducer arms in event-case order, persisted stable NodeIds, canonical
//! single-segment encoding, `programHash` patched over zeroed-hash bytes.
//!
//! v0.1 lowering subset: single-store programs (multi-store binding syntax
//! lands in v0.2a); `match` in views without payload binds; effects as linear
//! plans (`let x = svc…` = call step, remaining steps run on Ok; a `match` on
//! a call result with simple dispatch arms becomes onOk/onErr). Anything
//! outside the subset reports `NX0501 LoweringUnsupported` — never silent.

mod exprs;
mod queries;
mod views;

use crate::ast::{Decl, File, Stmt, TypeExpr};
use crate::check::Model;
use crate::diag::{DiagCode, Diagnostic, Span};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec::Vec,
};
use nexus_dsl_ir::ui_ir_capnp as ir;
use nexus_dsl_ir::{hashing, DIGEST_LEN, SCHEMA_MAJOR, SCHEMA_MINOR};

/// Default program budgets (v1.0): view nodes, expr nodes, list len, str len,
/// effect steps, locals, children.
pub const DEFAULT_BUDGETS: (u32, u32, u32, u32, u32, u32, u32) =
    (4096, 1024, 1024, 4096, 16, 32, 64);

pub struct Lowered {
    /// Canonical single-segment `.nxir` bytes (hash patched).
    pub nxir: Vec<u8>,
    pub program_hash: [u8; DIGEST_LEN],
}

/// Lowers a checked file. `source` is the canonical (formatted) source text —
/// its digest becomes `sourceDigest`.
///
/// # Errors
/// The first construct outside the v0.1 lowering subset.
pub fn lower_file(file: &File, model: &Model<'_>, source: &str) -> Result<Lowered, Diagnostic> {
    lower_file_with_catalog(file, model, source, &BTreeMap::new())
}

/// Like [`lower_file`], with the app's DEFAULT-locale catalog (key → display
/// text, from `i18n/<default>.json`) BAKED into the program: each `I18nKey`
/// entry points at the translated symbol instead of the dotted key, so `@t()`
/// renders real text with the existing `IdentityLocale` runtime. Runtime
/// locale SWITCHING (loading other catalogs) is the TASK-0081 i18n pipeline;
/// this is the default-locale floor (keys never leak to the screen).
///
/// # Errors
/// The first construct outside the v0.1 lowering subset.
pub fn lower_file_with_catalog(
    file: &File,
    model: &Model<'_>,
    source: &str,
    catalog: &BTreeMap<String, String>,
) -> Result<Lowered, Diagnostic> {
    let ctx = Ctx::build(file, model, catalog)?;

    // Build with a zeroed hash, canonicalize, hash, rebuild with the real
    // hash, canonicalize again → deterministic final bytes.
    let zero = [0u8; DIGEST_LEN];
    let first = build_message(&ctx, model, source, &zero)?;
    let hash = {
        let reader = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&first)
            .map_err(|_| internal(Span::default(), "self-read of freshly built IR failed"))?;
        let root = reader
            .root()
            .map_err(|_| internal(Span::default(), "self-read of freshly built IR failed"))?;
        hashing::compute_program_hash(root)
            .map_err(|_| internal(Span::default(), "hashing freshly built IR failed"))?
    };
    let nxir = build_message(&ctx, model, source, &hash)?;
    Ok(Lowered { nxir, program_hash: hash })
}

fn internal(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagCode::LoweringUnsupported, span, message.to_string())
}

pub(super) fn unsupported(span: Span, what: &str) -> Diagnostic {
    Diagnostic::new(
        DiagCode::LoweringUnsupported,
        span,
        format!("{what} is outside the v0.1 lowering subset"),
    )
}

/// Interning + canonical ordering context shared by the lowering walkers.
pub(super) struct Ctx<'a> {
    pub symbols: Vec<String>,
    symbol_ids: BTreeMap<String, u32>,
    /// Store indices in canonical (name-sorted) order → model index.
    pub store_order: Vec<usize>,
    pub event_order: Vec<usize>,
    /// Canonical component list: (name, Page? model idx : Component model idx).
    pub component_order: Vec<(&'a str, ComponentSource)>,
    pub component_index: BTreeMap<&'a str, u32>,
    pub event_index: BTreeMap<&'a str, u32>,
    pub i18n_keys: Vec<String>,
    /// Per `i18n_keys` index: the DISPLAY text the key's IR entry points at —
    /// the default-locale translation when the catalog has one, else the key
    /// itself (the pre-catalog behavior).
    pub i18n_texts: Vec<String>,
    pub entry_page: u32,
    /// Case name → (canonical event index, case index). Unambiguous only.
    case_map: BTreeMap<String, (u32, u32)>,
    /// Store field name → canonical store index. `Err(())` = the name exists
    /// in more than one store (ambiguous — using it is a lowering error).
    field_store: BTreeMap<String, Result<u32, ()>>,
    /// Component-owned implicit stores (`state:` blocks), appended AFTER the
    /// named stores in canonical order: (component name, canonical store
    /// index, model component index).
    pub local_stores: Vec<(String, u32, usize)>,
    /// Query declarations in canonical (name-sorted) order → model index.
    pub query_order: Vec<usize>,
    pub query_index: BTreeMap<&'a str, u32>,
    /// The declarations themselves, canonical order (index = spec id).
    pub queries: Vec<&'a crate::ast::QueryDecl>,
}

#[derive(Clone, Copy)]
pub(super) enum ComponentSource {
    Page(usize),
    Component(usize),
}

impl<'a> Ctx<'a> {
    fn build(
        file: &'a File,
        model: &Model<'a>,
        catalog: &BTreeMap<String, String>,
    ) -> Result<Self, Diagnostic> {
        // ---- collect every symbol the program mentions
        let mut set: BTreeSet<String> = BTreeSet::new();
        let mut i18n: BTreeSet<String> = BTreeSet::new();
        collect_symbols(file, &mut set, &mut i18n);

        // Default-locale texts join the symbol table so the i18n key entries
        // can point at them (`lower_file_with_catalog`).
        let i18n_texts: Vec<String> = i18n
            .iter()
            .map(|key| catalog.get(key).cloned().unwrap_or_else(|| key.clone()))
            .collect();
        for text in &i18n_texts {
            set.insert(text.clone());
        }

        let symbols: Vec<String> = set.into_iter().collect();
        let symbol_ids: BTreeMap<String, u32> =
            symbols.iter().enumerate().map(|(i, s)| (s.clone(), i as u32)).collect();

        // ---- canonical orders (sorted by name; source order never leaks)
        let mut store_order: Vec<usize> = (0..model.stores.len()).collect();
        store_order.sort_by_key(|&i| model.stores[i].name.text.as_str());
        let mut event_order: Vec<usize> = (0..model.events.len()).collect();
        event_order.sort_by_key(|&i| model.events[i].name.text.as_str());
        let mut query_order: Vec<usize> = (0..model.queries.len()).collect();
        query_order.sort_by_key(|&i| model.queries[i].name.text.as_str());
        let query_index: BTreeMap<&str, u32> = query_order
            .iter()
            .enumerate()
            .map(|(i, &m)| (model.queries[m].name.text.as_str(), i as u32))
            .collect();

        let mut component_order: Vec<(&str, ComponentSource)> = Vec::new();
        for (i, page) in model.pages.iter().enumerate() {
            component_order.push((page.name.text.as_str(), ComponentSource::Page(i)));
        }
        for (i, component) in model.components.iter().enumerate() {
            component_order.push((component.name.text.as_str(), ComponentSource::Component(i)));
        }
        component_order.sort_by_key(|(name, _)| *name);

        let component_index: BTreeMap<&str, u32> = component_order
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (*name, i as u32))
            .collect();
        let event_index: BTreeMap<&str, u32> = event_order
            .iter()
            .enumerate()
            .map(|(i, &m)| (model.events[m].name.text.as_str(), i as u32))
            .collect();
        // ---- entry page: the "/" route target, else the first page (sorted)
        let entry_page = model
            .routes
            .iter()
            .find(|route| route.path == "/")
            .and_then(|route| component_index.get(route.page.text.as_str()).copied())
            .or_else(|| {
                component_order
                    .iter()
                    .position(|(_, src)| matches!(src, ComponentSource::Page(_)))
                    .map(|i| i as u32)
            })
            .unwrap_or(0);

        // Case name → (canonical event idx, case idx), unambiguous entries only.
        let mut case_map: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for (canonical_idx, &model_idx) in event_order.iter().enumerate() {
            let event = model.events[model_idx];
            for (case_idx, case) in event.cases.iter().enumerate() {
                if matches!(model.case_lookup.get(case.name.text.as_str()), Some(&(e, c)) if e != usize::MAX && c != usize::MAX)
                {
                    case_map.insert(
                        case.name.text.clone(),
                        (canonical_idx as u32, case_idx as u32),
                    );
                }
            }
        }

        // Field name → owning store (canonical index); duplicates poison.
        let mut field_store: BTreeMap<String, Result<u32, ()>> = BTreeMap::new();
        for (canonical_idx, &model_idx) in store_order.iter().enumerate() {
            for field in &model.stores[model_idx].fields {
                field_store
                    .entry(field.name.text.clone())
                    .and_modify(|entry| *entry = Err(()))
                    .or_insert(Ok(canonical_idx as u32));
            }
        }

        // Component `state:` blocks → implicit stores appended after the
        // named ones (sorted by component name — deterministic). v1 rule:
        // a stateful component is instantiated exactly ONCE (per-instance
        // keyed storage rides with the retained-instance work) — enforced
        // below by the usage counter.
        let mut local_stores: Vec<(String, u32, usize)> = Vec::new();
        {
            let mut stateful: Vec<usize> = model
                .components
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.state.is_empty())
                .map(|(i, _)| i)
                .collect();
            stateful.sort_by_key(|&i| model.components[i].name.text.as_str());
            for &comp_idx in &stateful {
                let component = model.components[comp_idx];
                let store_idx = (store_order.len() + local_stores.len()) as u32;
                // Local fields participate in global resolution too (so
                // bindings/deps work); collisions poison like store fields.
                for field in &component.state {
                    field_store
                        .entry(field.name.text.clone())
                        .and_modify(|entry| *entry = Err(()))
                        .or_insert(Ok(store_idx));
                }
                local_stores.push((component.name.text.clone(), store_idx, comp_idx));
                let usage = count_component_usage(model, &component.name.text);
                if usage != 1 {
                    return Err(unsupported(
                        component.name.span,
                        "a stateful component instantiated other than exactly once                          (per-instance local state lands with the retained-instance work)",
                    ));
                }
            }
        }

        Ok(Self {
            symbols,
            symbol_ids,
            store_order,
            event_order,
            component_order,
            component_index,
            event_index,
            i18n_keys: i18n.into_iter().collect(),
            i18n_texts,
            entry_page,
            case_map,
            field_store,
            local_stores,
            queries: query_order.iter().map(|&i| model.queries[i]).collect(),
            query_order,
            query_index,
        })
    }

    /// (canonical event index, case index) for an unambiguous case name.
    pub(super) fn event_case(&self, case: &str) -> Option<(u32, u32)> {
        self.case_map.get(case).copied()
    }

    /// The canonical store owning `field`. `Err(true)` = ambiguous across
    /// stores, `Err(false)` = unknown field.
    pub(super) fn store_of_field(&self, field: &str) -> Result<u32, bool> {
        match self.field_store.get(field) {
            Some(Ok(store)) => Ok(*store),
            Some(Err(())) => Err(true),
            None => Err(false),
        }
    }

    pub(super) fn sym(&self, text: &str) -> u32 {
        // Every name was collected up front; a miss is a lowering bug caught
        // by tests, but stay total: fall back to 0 deterministically.
        self.symbol_ids.get(text).copied().unwrap_or(0)
    }

    pub(super) fn i18n_index(&self, key: &str) -> u32 {
        self.i18n_keys.binary_search_by(|k| k.as_str().cmp(key)).map_or(0, |i| i as u32)
    }
}

fn collect_symbols(file: &File, set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
    use crate::ast::{Expr, HandlerAction, ViewNode};

    fn walk_expr(expr: &Expr, set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
        match expr {
            Expr::EnumLit { ty, case, args, .. } => {
                set.insert(ty.text.clone());
                set.insert(case.text.clone());
                for arg in args {
                    walk_expr(arg, set, i18n);
                }
            }
            Expr::StateRef { path, .. }
            | Expr::PropsRef { path, .. }
            | Expr::DeviceRef { path, .. }
            | Expr::Path { segments: path, .. } => {
                for seg in path {
                    set.insert(seg.text.clone());
                }
            }
            Expr::Call { path, args, .. } => {
                for seg in path {
                    set.insert(seg.text.clone());
                }
                for arg in args {
                    walk_expr(&arg.value, set, i18n);
                }
            }
            Expr::I18n { key, args, .. } => {
                i18n.insert(key.clone());
                set.insert(key.clone());
                for arg in args {
                    walk_expr(arg, set, i18n);
                }
            }
            Expr::List { items, .. } => {
                for item in items {
                    walk_expr(item, set, i18n);
                }
            }
            Expr::Unary { operand, .. } => walk_expr(operand, set, i18n),
            Expr::Binary { lhs, rhs, .. } => {
                walk_expr(lhs, set, i18n);
                walk_expr(rhs, set, i18n);
            }
            _ => {}
        }
    }

    fn walk_stmts(stmts: &[Stmt], set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
        for stmt in stmts {
            match stmt {
                Stmt::Assign { path, value, .. } => {
                    for seg in path {
                        set.insert(seg.text.clone());
                    }
                    walk_expr(value, set, i18n);
                }
                Stmt::Let { name, value, .. } => {
                    set.insert(name.text.clone());
                    walk_expr(value, set, i18n);
                }
                Stmt::If { cond, then, els, .. } => {
                    walk_expr(cond, set, i18n);
                    walk_stmts(then, set, i18n);
                    walk_stmts(els, set, i18n);
                }
                Stmt::Match { scrutinee, arms, .. } => {
                    walk_expr(scrutinee, set, i18n);
                    for arm in arms {
                        set.insert(arm.pattern.case.text.clone());
                        for bind in &arm.pattern.binds {
                            set.insert(bind.text.clone());
                        }
                        walk_stmts(&arm.body, set, i18n);
                    }
                }
                Stmt::Dispatch { case, args, .. } => {
                    set.insert(case.text.clone());
                    for arg in args {
                        walk_expr(arg, set, i18n);
                    }
                }
                Stmt::ExprStmt { expr, .. } => walk_expr(expr, set, i18n),
            }
        }
    }

    fn walk_type(ty: &TypeExpr, set: &mut BTreeSet<String>) {
        set.insert(ty.name.text.clone());
        for arg in &ty.args {
            walk_type(arg, set);
        }
    }

    fn walk_view(node: &ViewNode, set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
        match node {
            ViewNode::Widget(widget) => {
                set.insert(widget.name.text.clone());
                // Auto-bind triggers: a $state-bound primary prop on an
                // interactive kind synthesizes a bind handler at lowering —
                // its trigger symbol must exist.
                for (name, value) in &widget.props {
                    if matches!(value, Expr::StateRef { .. }) {
                        match (widget.name.text.as_str(), name.text.as_str()) {
                            ("Toggle" | "Checkbox", "checked") => {
                                set.insert(alloc::string::String::from("Tap"));
                            }
                            ("TextField", "value") | ("TextArea", "value") => {
                                set.insert(alloc::string::String::from("Change"));
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(positional) = &widget.positional {
                    walk_expr(positional, set, i18n);
                    // The positional sugar becomes the registry primary prop
                    // during lowering — intern its name here or the emitted
                    // PropInit references a dangling symbol.
                    if let Some(primary) = crate::registry::widget_spec(&widget.name.text)
                        .and_then(|spec| spec.primary_prop)
                    {
                        set.insert(alloc::string::String::from(primary));
                    }
                }
                for (name, value) in &widget.props {
                    set.insert(name.text.clone());
                    walk_expr(value, set, i18n);
                }
                for modifier in &widget.modifiers {
                    for arg in &modifier.args {
                        walk_expr(&arg.value, set, i18n);
                    }
                }
                for handler in &widget.handlers {
                    set.insert(handler.trigger.text.clone());
                    match &handler.action {
                        HandlerAction::Dispatch { case, args } => {
                            set.insert(case.text.clone());
                            for arg in args {
                                walk_expr(arg, set, i18n);
                            }
                        }
                        HandlerAction::Emit { prop, args } => {
                            walk_expr(prop, set, i18n);
                            for arg in args {
                                walk_expr(arg, set, i18n);
                            }
                        }
                        HandlerAction::Navigate { path } => walk_expr(path, set, i18n),
                    }
                }
                for child in &widget.children {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::If { arms, els, .. } => {
                for (cond, body) in arms {
                    walk_expr(cond, set, i18n);
                    for child in body {
                        walk_view(child, set, i18n);
                    }
                }
                for child in els {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::For { var, iter, body, .. } => {
                set.insert(var.text.clone());
                walk_expr(iter, set, i18n);
                for child in body {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::Collection(collection) => {
                set.insert(collection.kind.text.clone());
                set.insert(collection.var.text.clone());
                walk_expr(&collection.binding, set, i18n);
                for modifier in &collection.modifiers {
                    for arg in &modifier.args {
                        walk_expr(&arg.value, set, i18n);
                    }
                }
                for child in &collection.body {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::Match { scrutinee, arms, .. } => {
                walk_expr(scrutinee, set, i18n);
                for arm in arms {
                    set.insert(arm.pattern.case.text.clone());
                    for child in &arm.body {
                        walk_view(child, set, i18n);
                    }
                }
            }
        }
    }

    for decl in &file.decls {
        match decl {
            Decl::Store(store) => {
                set.insert(store.name.text.clone());
                for field in &store.fields {
                    set.insert(field.name.text.clone());
                    walk_type(&field.ty, set);
                    if let Some(default) = &field.default {
                        walk_expr(default, set, i18n);
                    }
                }
            }
            Decl::Event(event) => {
                set.insert(event.name.text.clone());
                for case in &event.cases {
                    set.insert(case.name.text.clone());
                    for ty in &case.payload {
                        walk_type(ty, set);
                    }
                }
            }
            Decl::Reduce(reduce) => {
                set.insert(reduce.event.text.clone());
                for arm in &reduce.arms {
                    set.insert(arm.pattern.case.text.clone());
                    for bind in &arm.pattern.binds {
                        set.insert(bind.text.clone());
                    }
                    walk_stmts(&arm.body, set, i18n);
                }
            }
            Decl::Effect(effect) => {
                set.insert(effect.trigger.case.text.clone());
                for bind in &effect.trigger.binds {
                    set.insert(bind.text.clone());
                }
                walk_stmts(&effect.body, set, i18n);
            }
            Decl::Page(page) => {
                set.insert(page.name.text.clone());
                walk_view(&page.view, set, i18n);
            }
            Decl::Component(component) => {
                set.insert(component.name.text.clone());
                for prop in &component.props {
                    set.insert(prop.name.text.clone());
                    walk_type(&prop.ty, set);
                }
                for field in &component.state {
                    set.insert(field.name.text.clone());
                    walk_type(&field.ty, set);
                    if let Some(default) = &field.default {
                        walk_expr(default, set, i18n);
                    }
                    // Implicit store name symbol.
                    set.insert(alloc::format!("__local_{}", component.name.text));
                }
                walk_view(&component.view, set, i18n);
            }
            Decl::Routes(routes) => {
                for route in &routes.routes {
                    set.insert(route.page.text.clone());
                    for (name, ty) in &route.params {
                        set.insert(name.text.clone());
                        walk_type(ty, set);
                    }
                }
            }
            Decl::Query(query) => {
                set.insert(query.name.text.clone());
                set.insert(query.source.text.clone());
                set.insert(query.order_col.text.clone());
                for param in &query.params {
                    set.insert(param.name.text.clone());
                    walk_type(&param.ty, set);
                }
                for pred in &query.preds {
                    set.insert(pred.col.text.clone());
                    walk_expr(&pred.value, set, i18n);
                }
            }
            // Window intent carries no symbols or i18n keys (enum fields only).
            Decl::Window(_) => {}
        }
    }
}

/// Counts `componentRef` instantiations of `name` across every view.
/// A use inside a collection template counts as many (dynamic).
fn count_component_usage(model: &Model<'_>, name: &str) -> usize {
    fn walk(node: &crate::ast::ViewNode, name: &str, in_collection: bool) -> usize {
        use crate::ast::ViewNode as V;
        match node {
            V::Widget(widget) => {
                let own = if widget.name.text == name {
                    if in_collection { 2 } else { 1 }
                } else {
                    0
                };
                own + widget
                    .children
                    .iter()
                    .map(|child| walk(child, name, in_collection))
                    .sum::<usize>()
            }
            V::If { arms, els, .. } => {
                arms.iter()
                    .flat_map(|(_, body)| body.iter())
                    .chain(els.iter())
                    .map(|child| walk(child, name, in_collection))
                    .sum()
            }
            V::For { body, .. } => {
                body.iter().map(|child| walk(child, name, true)).sum()
            }
            V::Collection(collection) => {
                collection.body.iter().map(|child| walk(child, name, true)).sum()
            }
            V::Match { arms, .. } => arms
                .iter()
                .flat_map(|arm| arm.body.iter())
                .map(|child| walk(child, name, in_collection))
                .sum(),
        }
    }
    model
        .pages
        .iter()
        .map(|p| walk(&p.view, name, false))
        .chain(model.components.iter().map(|c| walk(&c.view, name, false)))
        .sum()
}

/// Builds the full message with the given `programHash` value and returns the
/// canonical single-segment bytes.
fn build_message(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    source: &str,
    hash: &[u8; DIGEST_LEN],
) -> Result<Vec<u8>, Diagnostic> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut program = message.init_root::<ir::ui_program::Builder<'_>>();
        program.set_schema_version_major(SCHEMA_MAJOR);
        program.set_schema_version_minor(SCHEMA_MINOR);
        program.set_program_hash(hash);
        program.set_source_digest(&hashing::sha256(source.as_bytes()));
        program.set_entry_page(ctx.entry_page);

        {
            // App-owned window intent. Absent = the default-initialized struct
            // (titlebar/auto/normal/false) — the ordinary-window default. The
            // compositor composes the frame under the active windowing policy
            // (docs/dev/ui/patterns/windowing/window-intent.md).
            use crate::ast::{WindowLevel, WindowMode, WindowStyle};
            let mut w = program.reborrow().init_window();
            if let Some(intent) = model.window {
                w.set_style(match intent.style {
                    WindowStyle::Titlebar => ir::WindowStyle::Titlebar,
                    WindowStyle::HiddenTitlebar => ir::WindowStyle::HiddenTitlebar,
                    WindowStyle::Plain => ir::WindowStyle::Plain,
                });
                w.set_mode(match intent.mode {
                    WindowMode::Auto => ir::WindowMode::Auto,
                    WindowMode::Freeform => ir::WindowMode::Freeform,
                    WindowMode::Fullscreen => ir::WindowMode::Fullscreen,
                });
                w.set_level(match intent.level {
                    WindowLevel::Normal => ir::WindowLevel::Normal,
                    WindowLevel::Desktop => ir::WindowLevel::Desktop,
                    WindowLevel::Overlay => ir::WindowLevel::Overlay,
                });
                w.set_resizable(intent.resizable);
            }
        }

        {
            let mut symbols = program.reborrow().init_symbols(ctx.symbols.len() as u32);
            for (i, symbol) in ctx.symbols.iter().enumerate() {
                symbols.set(i as u32, capnp::text::Reader::from(symbol.as_str()));
            }
        }
        {
            let (view_nodes, expr_nodes, list_len, str_len, steps, locals, children) =
                DEFAULT_BUDGETS;
            let mut budgets = program.reborrow().init_budgets();
            budgets.set_max_view_nodes(view_nodes);
            budgets.set_max_expr_nodes(expr_nodes);
            budgets.set_max_list_len(list_len);
            budgets.set_max_str_len(str_len);
            budgets.set_max_effect_steps(steps);
            budgets.set_max_locals(locals);
            budgets.set_max_children(children);
        }

        views::build_state(ctx, model, &mut program)?;
        views::build_components(ctx, model, &mut program)?;
        views::build_routes(ctx, model, &mut program)?;
        queries::build_query_specs(ctx, model, &mut program)?;

        {
            let mut keys = program.reborrow().init_i18n_keys(ctx.i18n_keys.len() as u32);
            for i in 0..ctx.i18n_keys.len() {
                let mut entry = keys.reborrow().get(i as u32);
                // Points at the DISPLAY text (default-locale translation when
                // the catalog has one, else the dotted key) — `IdentityLocale`
                // then renders real text instead of leaking keys to the UI.
                entry.set_key(ctx.sym(&ctx.i18n_texts[i]));
                entry.init_arg_types(0);
            }
        }
    }

    // Canonicalize to a single segment. `set_root_canonical` asserts the
    // output landed in ONE segment, so the first segment must be big enough
    // up front — size it from the working message (default-allocator growth
    // would add a second segment and abort on larger programs, e.g. the
    // 0080B shell).
    let total_words: usize = message
        .get_segments_for_output()
        .iter()
        .map(|segment| segment.len() / 8)
        .sum();
    let mut canonical = capnp::message::Builder::new(
        capnp::message::HeapAllocator::new()
            .first_segment_words(u32::try_from(total_words + 64).unwrap_or(u32::MAX)),
    );
    canonical
        .set_root_canonical(
            message
                .get_root_as_reader::<ir::ui_program::Reader<'_>>()
                .map_err(|_| internal(Span::default(), "canonicalize: reread failed"))?,
        )
        .map_err(|_| internal(Span::default(), "canonicalize failed"))?;
    let segments = canonical.get_segments_for_output();
    if segments.len() != 1 {
        return Err(internal(Span::default(), "canonical form is not single-segment"));
    }
    Ok(segments[0].to_vec())
}
