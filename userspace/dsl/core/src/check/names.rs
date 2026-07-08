// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Model building + name resolution (duplicates, unknown references,
//! route/page wiring, widget-vs-component resolution, modifier catalog).

use super::Model;
use crate::ast::{
    Decl, Expr, File, HandlerAction, ModifierCall, Pattern, Stmt, ViewNode,
};
use crate::diag::{DiagCode, Diagnostic, Span};
use crate::registry;
use alloc::{collections::BTreeMap, format, string::String, vec::Vec};

pub(super) fn build_model<'a>(file: &'a File, diags: &mut Vec<Diagnostic>) -> Model<'a> {
    let mut model = Model {
        stores: Vec::new(),
        events: Vec::new(),
        reduces: Vec::new(),
        effects: Vec::new(),
        pages: Vec::new(),
        components: Vec::new(),
        routes: Vec::new(),
        queries: Vec::new(),
        store_by_name: BTreeMap::new(),
        event_by_name: BTreeMap::new(),
        page_by_name: BTreeMap::new(),
        component_by_name: BTreeMap::new(),
        query_by_name: BTreeMap::new(),
        case_lookup: BTreeMap::new(),
        i18n_keys: Vec::new(),
        window: None,
    };

    let dup = |diags: &mut Vec<Diagnostic>, span: Span, kind: &str, name: &str| {
        diags.push(Diagnostic::new(
            DiagCode::DuplicateDefinition,
            span,
            format!("{kind} `{name}` is defined twice"),
        ));
    };

    for decl in &file.decls {
        match decl {
            Decl::Store(store) => {
                if model.store_by_name.insert(&store.name.text, model.stores.len()).is_some() {
                    dup(diags, store.name.span, "store", &store.name.text);
                }
                model.stores.push(store);
            }
            Decl::Event(event) => {
                let event_idx = model.events.len();
                if model.event_by_name.insert(&event.name.text, event_idx).is_some() {
                    dup(diags, event.name.span, "event type", &event.name.text);
                }
                for (case_idx, case) in event.cases.iter().enumerate() {
                    let entry = model.case_lookup.entry(&case.name.text);
                    use alloc::collections::btree_map::Entry;
                    match entry {
                        Entry::Vacant(slot) => {
                            slot.insert((event_idx, case_idx));
                        }
                        Entry::Occupied(mut slot) => {
                            // Ambiguous across events — poison + report.
                            slot.insert((usize::MAX, usize::MAX));
                            dup(diags, case.name.span, "event case", &case.name.text);
                        }
                    }
                }
                model.events.push(event);
            }
            Decl::Reduce(reduce) => model.reduces.push(reduce),
            Decl::Effect(effect) => model.effects.push(effect),
            Decl::Page(page) => {
                if model.page_by_name.insert(&page.name.text, model.pages.len()).is_some() {
                    dup(diags, page.name.span, "page", &page.name.text);
                }
                model.pages.push(page);
            }
            Decl::Component(component) => {
                if model
                    .component_by_name
                    .insert(&component.name.text, model.components.len())
                    .is_some()
                {
                    dup(diags, component.name.span, "component", &component.name.text);
                }
                model.components.push(component);
            }
            Decl::Routes(routes) => {
                for route in &routes.routes {
                    model.routes.push(route);
                }
            }
            Decl::Query(query) => {
                if model.query_by_name.insert(&query.name.text, model.queries.len()).is_some()
                {
                    dup(diags, query.name.span, "query", &query.name.text);
                }
                model.queries.push(query);
            }
            Decl::Window(window) => {
                if model.window.is_some() {
                    dup(diags, window.span, "Window", "Window");
                } else {
                    model.window = Some(window);
                }
            }
        }
    }
    model
}

pub(super) fn check_references(file: &File, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    // reduce/effect wiring
    let mut seen_reduce: BTreeMap<&str, ()> = BTreeMap::new();
    for reduce in &model.reduces {
        if !model.event_by_name.contains_key(reduce.event.text.as_str()) {
            diags.push(Diagnostic::new(
                DiagCode::UnknownEvent,
                reduce.event.span,
                format!("`reduce {}` references an unknown event type", reduce.event.text),
            ));
        }
        if seen_reduce.insert(&reduce.event.text, ()).is_some() {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                reduce.event.span,
                format!("`reduce {}` is defined twice", reduce.event.text),
            ));
        }
        // Arm patterns + exhaustiveness.
        if let Some(&event_idx) = model.event_by_name.get(reduce.event.text.as_str()) {
            let event = model.events[event_idx];
            let mut covered: Vec<bool> = alloc::vec![false; event.cases.len()];
            for arm in &reduce.arms {
                match event.cases.iter().position(|c| c.name.text == arm.pattern.case.text) {
                    Some(case_idx) => {
                        covered[case_idx] = true;
                        check_binds_arity(&arm.pattern, event.cases[case_idx].payload.len(), diags);
                    }
                    None => diags.push(Diagnostic::new(
                        DiagCode::UnknownEnumCase,
                        arm.pattern.case.span,
                        format!(
                            "`{}` is not a case of `{}`",
                            arm.pattern.case.text, reduce.event.text
                        ),
                    )),
                }
            }
            if covered.iter().any(|&c| !c) {
                let missing: Vec<&str> = event
                    .cases
                    .iter()
                    .zip(&covered)
                    .filter(|(_, &c)| !c)
                    .map(|(case, _)| case.name.text.as_str())
                    .collect();
                diags.push(Diagnostic::new(
                    DiagCode::NotExhaustive,
                    reduce.span,
                    format!("`reduce {}` misses cases: {}", reduce.event.text, missing.join(", ")),
                ));
            }
        }
    }

    for effect in &model.effects {
        resolve_case(&effect.trigger, model, diags);
        check_stmts(&effect.body, model, diags);
    }
    for reduce in &model.reduces {
        for arm in &reduce.arms {
            check_stmts(&arm.body, model, diags);
        }
    }

    // routes
    let mut seen_paths: BTreeMap<&str, ()> = BTreeMap::new();
    for route in &model.routes {
        if !model.page_by_name.contains_key(route.page.text.as_str()) {
            diags.push(Diagnostic::new(
                DiagCode::UnknownName,
                route.page.span,
                format!("route target `{}` is not a Page", route.page.text),
            ));
        }
        if seen_paths.insert(route.path.as_str(), ()).is_some() {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateRoute,
                route.path_span,
                format!("route `{}` is declared twice", route.path),
            ));
        }
    }

    // queries (the v1 shape contract, docs/dev/dsl/db-queries.md)
    for query in &model.queries {
        check_query_decl(query, diags);
    }

    // views
    for page in &model.pages {
        check_view(&page.view, model, diags);
    }
    for component in &model.components {
        check_view(&component.view, model, diags);
    }
    let _ = file;
}

/// Bound on `limit` (the per-page budget; the service re-caps at its edge).
const MAX_QUERY_LIMIT: i64 = 1000;

fn check_query_decl(query: &crate::ast::QueryDecl, diags: &mut Vec<Diagnostic>) {
    let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
    for param in &query.params {
        if seen.insert(&param.name.text, ()).is_some() {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                param.name.span,
                format!("query param `{}` is defined twice", param.name.text),
            ));
        }
        if !matches!(param.ty.name.text.as_str(), "Bool" | "Int" | "Fx" | "Str") {
            diags.push(Diagnostic::new(
                DiagCode::UnknownType,
                param.ty.span,
                format!("query params are scalar (Bool/Int/Fx/Str), not `{}`", param.ty.name.text),
            ));
        }
    }
    for pred in &query.preds {
        match pred.op {
            crate::ast::BinOp::Eq => {}
            crate::ast::BinOp::Ge | crate::ast::BinOp::Le => {
                // v1 rule: ranges ride the order column's index.
                if pred.col.text != query.order_col.text {
                    diags.push(Diagnostic::new(
                        DiagCode::QueryShape,
                        pred.span,
                        format!(
                            "range predicates target the `orderBy` column (`{}`), not `{}`",
                            query.order_col.text, pred.col.text
                        ),
                    ));
                }
            }
            _ => diags.push(Diagnostic::new(
                DiagCode::QueryShape,
                pred.span,
                String::from("v1 comparisons are `==`, `>=`, `<=` (strict bounds land with the v2 builder)"),
            )),
        }
        let is_param_ref = matches!(
            &pred.value,
            Expr::Path { segments, .. }
                if segments.len() == 1
                    && query.params.iter().any(|p| p.name.text == segments[0].text)
        );
        let is_const = matches!(
            &pred.value,
            Expr::Bool { .. } | Expr::Int { .. } | Expr::Fx { .. } | Expr::Str { .. }
        );
        if !is_param_ref && !is_const {
            diags.push(Diagnostic::new(
                DiagCode::QueryShape,
                pred.value.span(),
                String::from("predicate values are literals or query params (queries are pure values)"),
            ));
        }
    }
    if query.limit <= 0 || query.limit > MAX_QUERY_LIMIT {
        diags.push(Diagnostic::new(
            DiagCode::QueryShape,
            query.limit_span,
            format!("`limit` must be 1..={MAX_QUERY_LIMIT}"),
        ));
    }
}

/// Validates a `QueryName(args…, token: t)` execution site: every declared
/// param passed exactly once by name; `token:` optional; nothing extra.
pub(super) fn check_query_call(
    query: &crate::ast::QueryDecl,
    args: &[crate::ast::CallArg],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let mut covered: Vec<bool> = alloc::vec![false; query.params.len()];
    for arg in args {
        let Some(name) = &arg.name else {
            diags.push(Diagnostic::new(
                DiagCode::WrongArity,
                arg.value.span(),
                String::from("query arguments are named (`param: value`)"),
            ));
            continue;
        };
        if name.text == "token" {
            continue;
        }
        match query.params.iter().position(|p| p.name.text == name.text) {
            Some(idx) if covered[idx] => diags.push(Diagnostic::new(
                DiagCode::WrongArity,
                name.span,
                format!("query param `{}` is passed twice", name.text),
            )),
            Some(idx) => covered[idx] = true,
            None => diags.push(Diagnostic::new(
                DiagCode::UnknownField,
                name.span,
                format!("`{}` has no param `{}`", query.name.text, name.text),
            )),
        }
    }
    if covered.iter().any(|&c| !c) {
        let missing: Vec<&str> = query
            .params
            .iter()
            .zip(&covered)
            .filter(|(_, &c)| !c)
            .map(|(p, _)| p.name.text.as_str())
            .collect();
        diags.push(Diagnostic::new(
            DiagCode::WrongArity,
            span,
            format!("`{}` misses params: {}", query.name.text, missing.join(", ")),
        ));
    }
}

fn check_binds_arity(pattern: &Pattern, payload_len: usize, diags: &mut Vec<Diagnostic>) {
    if !pattern.binds.is_empty() && pattern.binds.len() != payload_len {
        diags.push(Diagnostic::new(
            DiagCode::WrongArity,
            pattern.span,
            format!(
                "`{}` carries {payload_len} value(s) but the pattern binds {}",
                pattern.case.text,
                pattern.binds.len()
            ),
        ));
    }
}

/// Resolves a bare event-case reference (effect triggers, dispatch targets).
pub(super) fn resolve_case(
    pattern: &Pattern,
    model: &Model<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Option<(usize, usize)> {
    match model.case_lookup.get(pattern.case.text.as_str()) {
        Some(&(usize::MAX, _)) | Some(&(_, usize::MAX)) => None, // already reported as ambiguous
        Some(&(event_idx, case_idx)) => {
            let payload_len = model.events[event_idx].cases[case_idx].payload.len();
            check_binds_arity(pattern, payload_len, diags);
            Some((event_idx, case_idx))
        }
        None => {
            diags.push(Diagnostic::new(
                DiagCode::UnknownEvent,
                pattern.case.span,
                format!("`{}` is not a declared event case", pattern.case.text),
            ));
            None
        }
    }
}

fn check_stmts(stmts: &[Stmt], model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            Stmt::Dispatch { case, args, span } => {
                let pattern = Pattern { case: case.clone(), binds: Vec::new(), span: *span };
                if let Some((event_idx, case_idx)) = resolve_case(&pattern, model, diags) {
                    let payload_len = model.events[event_idx].cases[case_idx].payload.len();
                    if args.len() != payload_len {
                        diags.push(Diagnostic::new(
                            DiagCode::WrongArity,
                            *span,
                            format!(
                                "`{}` carries {payload_len} value(s) but dispatch passes {}",
                                case.text,
                                args.len()
                            ),
                        ));
                    }
                }
                for arg in args {
                    check_calls_in_expr(arg, model, diags);
                }
            }
            Stmt::If { cond, then, els, .. } => {
                check_calls_in_expr(cond, model, diags);
                check_stmts(then, model, diags);
                check_stmts(els, model, diags);
            }
            Stmt::Match { scrutinee, arms, .. } => {
                check_calls_in_expr(scrutinee, model, diags);
                for arm in arms {
                    check_stmts(&arm.body, model, diags);
                }
            }
            Stmt::Assign { value, .. } | Stmt::Let { value, .. } => {
                check_calls_in_expr(value, model, diags);
            }
            Stmt::ExprStmt { expr, .. } => check_calls_in_expr(expr, model, diags),
        }
    }
}

/// Walks an expression and validates every call site against the platform
/// surface: `svc.<service>.<method>` against the IDL-generated signature
/// table (unknown service/method/arity = stable diagnostics), a bare
/// `Name(args)` against the declared queries.
fn check_calls_in_expr(expr: &Expr, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    match expr {
        Expr::Call { path, args, span } => {
            if path.first().map(|s| s.text.as_str()) == Some("svc") && path.len() == 3 {
                check_svc_call(&path[1], &path[2], args, *span, diags);
            } else if path.len() == 1 {
                if let Some(&idx) = model.query_by_name.get(path[0].text.as_str()) {
                    check_query_call(model.queries[idx], args, *span, diags);
                }
            }
            for arg in args {
                check_calls_in_expr(&arg.value, model, diags);
            }
        }
        Expr::List { items, .. } | Expr::EnumLit { args: items, .. } => {
            for item in items {
                check_calls_in_expr(item, model, diags);
            }
        }
        Expr::I18n { args, .. } => {
            for arg in args {
                check_calls_in_expr(arg, model, diags);
            }
        }
        Expr::Unary { operand, .. } => check_calls_in_expr(operand, model, diags),
        Expr::Binary { lhs, rhs, .. } => {
            check_calls_in_expr(lhs, model, diags);
            check_calls_in_expr(rhs, model, diags);
        }
        _ => {}
    }
}

/// Signature check against the generated platform service surface
/// (`tools/nexus-idl/schemas/dsl_services.capnp` → `registry::svc_method`).
fn check_svc_call(
    service: &crate::ast::Ident,
    method: &crate::ast::Ident,
    args: &[crate::ast::CallArg],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    match crate::registry::svc_method(&service.text, &method.text) {
        crate::registry::SvcLookup::UnknownService => diags.push(Diagnostic::new(
            DiagCode::UnknownService,
            service.span,
            format!("`svc.{}` is not a platform service (see docs/dev/dsl/services.md)", service.text),
        )),
        crate::registry::SvcLookup::UnknownMethod => diags.push(Diagnostic::new(
            DiagCode::UnknownServiceMethod,
            method.span,
            format!("`svc.{}` has no method `{}`", service.text, method.text),
        )),
        crate::registry::SvcLookup::Found { arity } => {
            let positional = args
                .iter()
                .filter(|a| a.name.as_ref().map(|n| n.text.as_str()) != Some("timeoutMs"))
                .count();
            if positional != arity {
                diags.push(Diagnostic::new(
                    DiagCode::WrongArity,
                    span,
                    format!(
                        "`svc.{}.{}` takes {arity} argument(s), got {positional}",
                        service.text, method.text,
                    ),
                ));
            }
        }
    }
}

pub(super) fn check_view(node: &ViewNode, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    match node {
        ViewNode::Widget(widget) => {
            let is_widget = registry::widget_spec(&widget.name.text).is_some();
            let is_component = model.component_by_name.contains_key(widget.name.text.as_str());
            if !is_widget && !is_component {
                diags.push(Diagnostic::new(
                    DiagCode::UnknownWidget,
                    widget.name.span,
                    format!("`{}` is neither a widget nor a declared component", widget.name.text),
                ));
            }
            if is_component {
                if let Some(&idx) = model.component_by_name.get(widget.name.text.as_str()) {
                    let component = model.components[idx];
                    for (prop, _) in &widget.props {
                        if !component.props.iter().any(|p| p.name.text == prop.text) {
                            diags.push(Diagnostic::new(
                                DiagCode::UnknownField,
                                prop.span,
                                format!(
                                    "`{}` has no prop `{}`",
                                    widget.name.text, prop.text
                                ),
                            ));
                        }
                    }
                }
            }
            check_modifiers(&widget.modifiers, diags);
            for handler in &widget.handlers {
                if !registry::TRIGGERS.contains(&handler.trigger.text.as_str()) {
                    diags.push(Diagnostic::new(
                        DiagCode::UnknownName,
                        handler.trigger.span,
                        format!("unknown interaction `{}`", handler.trigger.text),
                    ));
                }
                if let HandlerAction::Navigate { path } = &handler.action {
                    // A literal path must match a declared route (dynamic
                    // paths resolve at runtime; unmatched = deterministic error).
                    if let Expr::Str { value, span } = path {
                        let matches_route = model.routes.iter().any(|route| {
                            let pat: Vec<&str> =
                                route.path.split('/').filter(|s| !s.is_empty()).collect();
                            let have: Vec<&str> =
                                value.split('/').filter(|s| !s.is_empty()).collect();
                            pat.len() == have.len()
                                && pat.iter().zip(&have).all(|(p, h)| {
                                    p.starts_with(':') || p == h
                                })
                        });
                        if !matches_route {
                            diags.push(Diagnostic::new(
                                DiagCode::UnknownName,
                                *span,
                                format!("`{value}` matches no declared route"),
                            ));
                        }
                    }
                }
                if let HandlerAction::Dispatch { case, args } = &handler.action {
                    let pattern =
                        Pattern { case: case.clone(), binds: Vec::new(), span: case.span };
                    if let Some((event_idx, case_idx)) = resolve_case(&pattern, model, diags) {
                        let payload_len =
                            model.events[event_idx].cases[case_idx].payload.len();
                        if args.len() != payload_len {
                            diags.push(Diagnostic::new(
                                DiagCode::WrongArity,
                                handler.span,
                                format!(
                                    "`{}` carries {payload_len} value(s) but dispatch passes {}",
                                    case.text,
                                    args.len()
                                ),
                            ));
                        }
                    }
                }
            }
            for child in &widget.children {
                check_view(child, model, diags);
            }
        }
        ViewNode::If { arms, els, .. } => {
            for (cond, body) in arms {
                check_device_expr(cond, diags);
                for child in body {
                    check_view(child, model, diags);
                }
            }
            for child in els {
                check_view(child, model, diags);
            }
        }
        ViewNode::For { body, .. } => {
            for child in body {
                check_view(child, model, diags);
            }
        }
        ViewNode::Collection(collection) => {
            check_modifiers(&collection.modifiers, diags);
            for child in &collection.body {
                check_view(child, model, diags);
            }
        }
        ViewNode::Match { arms, .. } => {
            for arm in arms {
                for child in &arm.body {
                    check_view(child, model, diags);
                }
            }
        }
    }
}

fn check_modifiers(modifiers: &[ModifierCall], diags: &mut Vec<Diagnostic>) {
    for modifier in modifiers {
        match registry::modifier_spec(&modifier.name.text) {
            None => diags.push(Diagnostic::new(
                DiagCode::UnknownModifier,
                modifier.name.span,
                format!("unknown modifier `.{}`", modifier.name.text),
            )),
            Some((_, spec)) => {
                if modifier.args.len() != spec.args.len() {
                    diags.push(Diagnostic::new(
                        DiagCode::WrongArity,
                        modifier.span,
                        format!(
                            "`.{}` takes {} argument(s), got {}",
                            spec.name,
                            spec.args.len(),
                            modifier.args.len()
                        ),
                    ));
                }
            }
        }
    }
}

/// Validates `device.*` field names + enum-like values in comparisons.
fn check_device_expr(expr: &Expr, diags: &mut Vec<Diagnostic>) {
    if let Expr::Binary { lhs, rhs, .. } = expr {
        check_device_expr(lhs, diags);
        check_device_expr(rhs, diags);
    }
    if let Expr::DeviceRef { path, span } = expr {
        let Some(first) = path.first() else { return };
        if registry::device_field(&first.text).is_none() {
            diags.push(Diagnostic::new(
                DiagCode::UnknownField,
                *span,
                format!("unknown device environment field `device.{}`", first.text),
            ));
        }
    }
}
