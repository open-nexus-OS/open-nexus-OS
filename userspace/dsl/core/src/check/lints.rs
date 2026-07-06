// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Opinionated rules (docs/dev/dsl/state.md#linterror-posture-v1):
//! reducer purity, collection keys, a11y labels, duplicate modifiers,
//! bounded `for`, profile-branch fallback, svc-result/timeout discipline.

use super::Model;
use crate::ast::{Expr, ModifierCall, Stmt, ViewNode, WidgetNode};
use crate::diag::{DiagCode, Diagnostic};
use crate::registry;
use alloc::{format, string::String, vec::Vec};

pub(super) fn run(file: &crate::ast::File, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    let _ = file;
    for reduce in &model.reduces {
        for arm in &reduce.arms {
            purity(&arm.body, diags);
        }
    }
    for effect in &model.effects {
        svc_discipline(&effect.body, diags);
    }
    for page in &model.pages {
        view_lints(&page.view, diags);
    }
    for component in &model.components {
        view_lints(&component.view, diags);
    }
}

// ------------------------------------------------------------ reducer purity

/// Reducers: no `svc.*`, no `dispatch`, no bare call statements.
fn purity(stmts: &[Stmt], diags: &mut Vec<Diagnostic>) {
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
                if let Some(span) = find_svc_call(value) {
                    diags.push(Diagnostic::new(
                        DiagCode::ReducerImpure,
                        span,
                        String::from("reducers are pure: `svc.*` belongs in an `@effect`"),
                    ));
                }
            }
            Stmt::If { then, els, cond, .. } => {
                if let Some(span) = find_svc_call(cond) {
                    diags.push(Diagnostic::new(
                        DiagCode::ReducerImpure,
                        span,
                        String::from("reducers are pure: `svc.*` belongs in an `@effect`"),
                    ));
                }
                purity(then, diags);
                purity(els, diags);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    purity(&arm.body, diags);
                }
            }
        }
    }
}

fn find_svc_call(expr: &Expr) -> Option<crate::diag::Span> {
    match expr {
        Expr::Call { path, span, args } => {
            if path.first().map(|seg| seg.text.as_str()) == Some("svc") {
                return Some(*span);
            }
            args.iter().find_map(|arg| find_svc_call(&arg.value))
        }
        Expr::Unary { operand, .. } => find_svc_call(operand),
        Expr::Binary { lhs, rhs, .. } => find_svc_call(lhs).or_else(|| find_svc_call(rhs)),
        Expr::List { items, .. } | Expr::EnumLit { args: items, .. } => {
            items.iter().find_map(find_svc_call)
        }
        Expr::I18n { args, .. } => args.iter().find_map(find_svc_call),
        _ => None,
    }
}

// -------------------------------------------------------- effect discipline

/// v0.1 posture (promoted to errors with the async-recipe wave): a service
/// call without `timeoutMs:` warns; an ignored `Result` (bare call statement)
/// warns.
fn svc_discipline(stmts: &[Stmt], diags: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { value, .. } => timeout_check(value, diags),
            Stmt::ExprStmt { expr, span } => {
                if find_svc_call(expr).is_some() {
                    diags.push(Diagnostic::new(
                        DiagCode::UnhandledResult,
                        *span,
                        String::from(
                            "the service result is ignored; bind it with `let` and handle it",
                        ),
                    ));
                }
                timeout_check(expr, diags);
            }
            Stmt::If { then, els, .. } => {
                svc_discipline(then, diags);
                svc_discipline(els, diags);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    svc_discipline(&arm.body, diags);
                }
            }
            Stmt::Assign { .. } | Stmt::Dispatch { .. } => {}
        }
    }
}

fn timeout_check(expr: &Expr, diags: &mut Vec<Diagnostic>) {
    if let Expr::Call { path, args, span } = expr {
        if path.first().map(|seg| seg.text.as_str()) == Some("svc")
            && !args.iter().any(|arg| {
                arg.name.as_ref().map(|n| n.text.as_str()) == Some("timeoutMs")
            })
        {
            diags.push(Diagnostic::new(
                DiagCode::MissingTimeout,
                *span,
                String::from("service calls should pass `timeoutMs:` explicitly"),
            ));
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
        ViewNode::For { .. } | ViewNode::Collection(_) => false,
    }
}
