// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The rules that are CONTRACTS rather than suggestions: reducer purity,
//! collection keys, a11y labels, duplicate modifiers, bounded `for`, the
//! service-result and retired-timeout discipline — and the one real warning
//! left, the profile-branch fallback.
//!
//! `docs/dev/dsl/principles.md` §4 is the authority: a violated contract "is an
//! error, NOT a lint suggestion". The rule this module exists for is §4's own
//! sentence — *"Effects must handle both `Ok` and `Err` of every service
//! call"* — written up for app authors in `docs/dev/dsl/services.md`.
//!
//! (The old pointer here named `state.md#linterror-posture-v1`, a section that
//! was consolidated away; the anchor had been dangling since.)

use super::Model;
use crate::ast::{Expr, ModifierCall, Stmt, ViewNode, WidgetNode};
use crate::diag::{DiagCode, Diagnostic};
use crate::registry;
use alloc::{format, string::String, vec::Vec};

pub(super) fn run(file: &crate::ast::File, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    let _ = file;
    for reduce in &model.reduces {
        for arm in &reduce.arms {
            purity(&arm.body, model, diags);
        }
    }
    for effect in &model.effects {
        effect_discipline(&effect.body, model, diags);
    }
    for page in &model.pages {
        view_lints(&page.view, diags);
    }
    for component in &model.components {
        view_lints(&component.view, diags);
    }
}

// ------------------------------------------------------------ reducer purity

/// Reducers: no `svc.*`, no query execution, no `dispatch`, no bare call
/// statements — the pure-build / effect-execute split (db-queries.md).
fn purity(stmts: &[Stmt], model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            Stmt::Dispatch { span, .. } => diags.push(Diagnostic::new(
                DiagCode::ReducerImpure,
                *span,
                String::from("reducers are pure: `dispatch` belongs in an `@effect`"),
            )),
            Stmt::ExprStmt { span, .. } => diags.push(Diagnostic::new(
                DiagCode::ReducerImpure,
                *span,
                String::from("reducers are pure: service calls belong in an `@effect`"),
            )),
            Stmt::Let { value, .. } | Stmt::Assign { value, .. } => {
                if let Some(span) = find_io_call(value, model) {
                    diags.push(Diagnostic::new(
                        DiagCode::ReducerImpure,
                        span,
                        String::from(
                            "reducers are pure: `svc.*`/query execution belongs in an `@effect`",
                        ),
                    ));
                }
            }
            Stmt::If { then, els, cond, .. } => {
                if let Some(span) = find_io_call(cond, model) {
                    diags.push(Diagnostic::new(
                        DiagCode::ReducerImpure,
                        span,
                        String::from(
                            "reducers are pure: `svc.*`/query execution belongs in an `@effect`",
                        ),
                    ));
                }
                purity(then, model, diags);
                purity(els, model, diags);
            }
            Stmt::Match { scrutinee, arms, .. } => {
                if let Some(span) = find_io_call(scrutinee, model) {
                    diags.push(Diagnostic::new(
                        DiagCode::ReducerImpure,
                        span,
                        String::from(
                            "reducers are pure: `svc.*`/query execution belongs in an `@effect`",
                        ),
                    ));
                }
                for arm in arms {
                    purity(&arm.body, model, diags);
                }
            }
        }
    }
}

/// IO call sites: `svc.*` and executions of a declared `Query`.
fn find_io_call(expr: &Expr, model: &Model<'_>) -> Option<crate::diag::Span> {
    match expr {
        Expr::Call { path, span, args } => {
            if path.first().map(|seg| seg.text.as_str()) == Some("svc") {
                return Some(*span);
            }
            if path.len() == 1 && model.query_by_name.contains_key(path[0].text.as_str()) {
                return Some(*span);
            }
            args.iter().find_map(|arg| find_io_call(&arg.value, model))
        }
        Expr::Unary { operand, .. } => find_io_call(operand, model),
        Expr::Binary { lhs, rhs, .. } => {
            find_io_call(lhs, model).or_else(|| find_io_call(rhs, model))
        }
        Expr::List { items, .. } | Expr::EnumLit { args: items, .. } => {
            items.iter().find_map(|item| find_io_call(item, model))
        }
        Expr::I18n { args, .. } => args.iter().find_map(|arg| find_io_call(arg, model)),
        _ => None,
    }
}

// -------------------------------------------------------- effect discipline

/// Effect discipline — ONE walk over every expression position in an effect
/// body, applying two contracts that both live here because both are about
/// what a service call may look like.
///
/// **§4 (`docs/dev/dsl/principles.md`): "Effects must handle both `Ok` and
/// `Err` of every service call."** So an IO call must be the SCRUTINEE of a
/// `match` that declares both arms; every other position is `NX0407`. This is
/// not a style rule. `lower/effects.rs` states the runtime semantics — *"a call
/// step binds its result on Ok and continues, dispatches `onErr` and stops on
/// Err"* — so a call with no `Err` arm STOPS THE PLAN SILENTLY: the `Loaded`
/// event never dispatches, `loading` is never cleared, and the app sits on a
/// spinner with no way to know (TASK-0077B P3).
///
/// Until P3 the rule saw only a bare call statement, which the whole corpus had
/// already stopped writing — 62 service calls, zero bare ones — while
/// `let r = svc.f();` and a one-armed `match` dropped their error path in
/// silence. The docs taught the `let` form two lines above the sentence it
/// breaks, which is where the two violations came from.
///
/// **P0's retired `timeoutMs:`** rides the same walk. It used to be applied to
/// `let` values and bare statements only — never to a match scrutinee, i.e.
/// never to the form 60 of 62 call sites use — so the argument P0 set out to
/// abolish was still silently accepted on the common path.
fn effect_discipline(stmts: &[Stmt], model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            // The ONE handled form: the call is the scrutinee, both paths declared.
            Stmt::Match { scrutinee, arms, span } if is_io_call(scrutinee, model) => {
                timeout_check(scrutinee, diags);
                // An IO call among the ARGUMENTS is a second result nobody handles.
                if let Some(nested) = nested_io_call(scrutinee, model) {
                    diags.push(unhandled_here(nested));
                }
                let declares = |case: &str| arms.iter().any(|a| a.pattern.case.text == case);
                let missing = match (declares("Ok"), declares("Err")) {
                    (true, true) => None,
                    (true, false) => Some(
                        "this service call declares no `Err` arm; on failure the effect stops \
                         silently and the app is never told",
                    ),
                    (false, true) => Some(
                        "this service call declares no `Ok` arm; on success the effect stops \
                         without using the result",
                    ),
                    (false, false) => Some(
                        "this service call declares neither `Ok` nor `Err`; both paths of the \
                         result must be handled",
                    ),
                };
                if let Some(message) = missing {
                    diags.push(Diagnostic::new(
                        DiagCode::UnhandledResult,
                        *span,
                        String::from(message),
                    ));
                }
                for arm in arms {
                    effect_discipline(&arm.body, model, diags);
                }
            }
            Stmt::Match { scrutinee, arms, .. } => {
                check_expr(scrutinee, model, diags);
                for arm in arms {
                    effect_discipline(&arm.body, model, diags);
                }
            }
            Stmt::Let { value, .. } | Stmt::Assign { value, .. } => {
                check_expr(value, model, diags);
            }
            Stmt::ExprStmt { expr, .. } => check_expr(expr, model, diags),
            Stmt::Dispatch { args, .. } => {
                for arg in args {
                    check_expr(arg, model, diags);
                }
            }
            Stmt::If { cond, then, els, .. } => {
                check_expr(cond, model, diags);
                effect_discipline(then, model, diags);
                effect_discipline(els, model, diags);
            }
        }
    }
}

/// An expression position that is NOT a handled match scrutinee: any IO call in
/// it drops its result.
fn check_expr(expr: &Expr, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    timeout_check(expr, diags);
    if let Some(span) = find_io_call(expr, model) {
        diags.push(unhandled_here(span));
    }
}

fn unhandled_here(span: crate::diag::Span) -> Diagnostic {
    Diagnostic::new(
        DiagCode::UnhandledResult,
        span,
        String::from(
            "a service result must be handled on BOTH paths: \
             `match <call> { Ok(v) => dispatch(..), Err(e) => dispatch(..), }` \
             — otherwise the effect stops silently when the call fails",
        ),
    )
}

/// Whether `expr` IS an IO call (not merely contains one): `svc.*`, or an
/// execution of a declared `Query`. Same two forms `find_io_call` looks for.
fn is_io_call(expr: &Expr, model: &Model<'_>) -> bool {
    match expr {
        Expr::Call { path, .. } => {
            path.first().map(|seg| seg.text.as_str()) == Some("svc")
                || (path.len() == 1 && model.query_by_name.contains_key(path[0].text.as_str()))
        }
        _ => false,
    }
}

/// An IO call INSIDE `expr` rather than `expr` itself — for a call, one hiding
/// in its arguments.
fn nested_io_call(expr: &Expr, model: &Model<'_>) -> Option<crate::diag::Span> {
    match expr {
        Expr::Call { args, .. } => args.iter().find_map(|arg| find_io_call(&arg.value, model)),
        other => find_io_call(other, model),
    }
}

/// A service call carries NO client timeout (TASK-0077B P0). The exchange ends
/// with the reply or with the service's death and nothing else (RFC-0093 §7,
/// RFC-0096) — the app-host stopped reading the number in TASK-0054C P2-a, so
/// the language stopped asking for it here. Passing one is an error: a number
/// that looks like a bound but is not one is worse than no number.
fn timeout_check(expr: &Expr, diags: &mut Vec<Diagnostic>) {
    if let Expr::Call { path, args, .. } = expr {
        if path.first().map(|seg| seg.text.as_str()) != Some("svc") {
            return;
        }
        for arg in args {
            if let Some(name) = arg.name.as_ref() {
                if name.text.as_str() == "timeoutMs" {
                    diags.push(Diagnostic::new(
                        DiagCode::RetiredTimeout,
                        name.span,
                        String::from(
                            "`timeoutMs:` is retired — a service call has no client clock; \
                             the reply or the service's death ends it",
                        ),
                    ));
                }
            }
        }
    }
}

// ---------------------------------------------------------------- view lints

fn view_lints(node: &ViewNode, diags: &mut Vec<Diagnostic>) {
    match node {
        ViewNode::Widget(widget) => {
            duplicate_modifiers(&widget.modifiers, diags);
            a11y_label(widget, diags);
            for child in &widget.children {
                view_lints(child, diags);
            }
            for binding in &widget.slot_bodies {
                for child in &binding.body {
                    view_lints(child, diags);
                }
            }
        }
        ViewNode::If { arms, els, span } => {
            // Profile-driven branching wants a fallback.
            let on_profile = arms.iter().any(|(cond, _)| mentions_device_profile(cond));
            if on_profile && els.is_empty() {
                diags.push(Diagnostic::new(
                    DiagCode::MissingProfileElse,
                    *span,
                    String::from(
                        "profile branch without a final `else`: a device you didn't \
                         think of gets nothing (add the default branch)",
                    ),
                ));
            }
            for (_, body) in arms {
                for child in body {
                    view_lints(child, diags);
                }
            }
            for child in els {
                view_lints(child, diags);
            }
        }
        ViewNode::For { iter, body, span, .. } => {
            // Static bound required: a literal list (or later, a capped range).
            if !matches!(iter, Expr::List { .. }) {
                diags.push(Diagnostic::new(
                    DiagCode::UnboundedFor,
                    *span,
                    String::from(
                        "`for` needs a statically bounded iterable (list literal); \
                         use `List(expr) { item in … }` for data-driven collections",
                    ),
                ));
            }
            for child in body {
                view_lints(child, diags);
            }
        }
        ViewNode::Collection(collection) => {
            duplicate_modifiers(&collection.modifiers, diags);
            // Every collection item template needs a stable key.
            for child in &collection.body {
                if !template_root_has_key(child) {
                    diags.push(Diagnostic::new(
                        DiagCode::MissingKey,
                        child.span(),
                        String::from(
                            "collection items need a stable `.key(expr)` on the template root",
                        ),
                    ));
                }
                view_lints(child, diags);
            }
        }
        ViewNode::Match { arms, .. } => {
            for arm in arms {
                for child in &arm.body {
                    view_lints(child, diags);
                }
            }
        }
        // A leaf: the caller's body nodes are linted at the callsite.
        ViewNode::Slot { .. } => {}
    }
}

fn duplicate_modifiers(modifiers: &[ModifierCall], diags: &mut Vec<Diagnostic>) {
    for (i, modifier) in modifiers.iter().enumerate() {
        if modifiers[..i].iter().any(|m| m.name.text == modifier.name.text) {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateModifier,
                modifier.span,
                format!("`.{}` is applied twice on the same node", modifier.name.text),
            ));
        }
    }
}

/// Interactive widgets need an accessible name: their label prop (or
/// positional primary that IS the label prop) or an explicit `.label(…)`.
fn a11y_label(widget: &WidgetNode, diags: &mut Vec<Diagnostic>) {
    let Some(spec) = registry::widget_spec(&widget.name.text) else { return };
    if !spec.interactive {
        return;
    }
    let has_label_modifier = widget.modifiers.iter().any(|m| m.name.text == "label");
    let has_label_prop = spec.label_prop.is_some_and(|label_prop| {
        widget.props.iter().any(|(name, _)| name.text == label_prop)
            || (widget.positional.is_some() && spec.primary_prop == Some(label_prop))
    });
    if !has_label_modifier && !has_label_prop {
        diags.push(Diagnostic::new(
            DiagCode::MissingLabel,
            widget.span,
            format!(
                "interactive `{}` needs an accessible name (a `{}:` prop or `.label(…)`)",
                widget.name.text,
                spec.label_prop.unwrap_or("label")
            ),
        ));
    }
}

fn mentions_device_profile(expr: &Expr) -> bool {
    match expr {
        Expr::DeviceRef { path, .. } => {
            path.first().map(|seg| seg.text.as_str()) == Some("profile")
        }
        Expr::Unary { operand, .. } => mentions_device_profile(operand),
        Expr::Binary { lhs, rhs, .. } => {
            mentions_device_profile(lhs) || mentions_device_profile(rhs)
        }
        _ => false,
    }
}

fn template_root_has_key(node: &ViewNode) -> bool {
    match node {
        ViewNode::Widget(widget) => widget.modifiers.iter().any(|m| m.name.text == "key"),
        // Conditional templates: every branch root must carry the key.
        ViewNode::If { arms, els, .. } => {
            arms.iter().all(|(_, body)| body.iter().all(template_root_has_key))
                && (els.is_empty() || els.iter().all(template_root_has_key))
        }
        ViewNode::Match { arms, .. } => {
            arms.iter().all(|arm| arm.body.iter().all(template_root_has_key))
        }
        // A slot placeholder cannot carry a key — the caller's body nodes do,
        // and they are checked at the callsite, in the caller's template.
        ViewNode::For { .. } | ViewNode::Collection(_) | ViewNode::Slot { .. } => false,
    }
}
