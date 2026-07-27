// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slot rules (RFC-0084): component content regions.
//!
//! Two laws are enforced here, because neither is emergent:
//!
//! - **Caller scope.** A slot body is lexically part of the CALLER, so
//!   `$props` inside it resolves against the ENCLOSING component's props.
//!   Lowering already does that (the body lowers in the caller's `Env`), but
//!   without a rule a body naming a prop of the *callee* would silently bind
//!   to the caller's — correct, and baffling. Rule 13 makes it an error.
//! - **No forwarding in v1.** `Slot x` inside a slot body would need a frame
//!   stack rather than a frame; the runtime makes it structurally impossible
//!   by clearing the frame, and rule 11 turns that silence into a diagnostic.
//!
//! Two pre-existing silent drops are closed on the way past: component
//! references discarded plain children (rule 9) and modifiers (rule 10)
//! without a word.

use super::Model;
use crate::ast::{ComponentDecl, Expr, SlotBinding, ViewNode, WidgetNode};
use crate::diag::{DiagCode, Diagnostic};
use alloc::{format, string::String, vec::Vec};

/// `Slot` never names a component — the placeholder must stay unambiguous.
const RESERVED_COMPONENT_NAMES: &[&str] = &["Slot"];

pub(super) fn check(file: &crate::ast::File, model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    let _ = file;

    for component in &model.components {
        if RESERVED_COMPONENT_NAMES.contains(&component.name.text.as_str()) {
            diags.push(Diagnostic::new(
                DiagCode::SlotShape,
                component.name.span,
                format!("`{}` is reserved — it is the slot placeholder", component.name.text),
            ));
        }
        check_declarations(component, diags);
        // Inside a component, `Slot x` must name one of its own slots.
        check_placeholders(&component.view, Some(component), model, diags);
    }
    // A page has neither props nor slots, so a placeholder there can never
    // bind (rule 6).
    for page in &model.pages {
        check_placeholders(&page.view, None, model, diags);
    }
}

/// Rules 7 + 8: a slot name is declared once and does not collide with a prop
/// or state field — `$props.x` and `Slot x` must never race for one name.
fn check_declarations(component: &ComponentDecl, diags: &mut Vec<Diagnostic>) {
    for (i, slot) in component.slots.iter().enumerate() {
        if component.slots[..i].iter().any(|earlier| earlier.text == slot.text) {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                slot.span,
                format!("slot `{}` is declared twice", slot.text),
            ));
        }
        if component.props.iter().any(|p| p.name.text == slot.text) {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                slot.span,
                format!("slot `{}` collides with a prop of the same name", slot.text),
            ));
        }
        if component.state.iter().any(|f| f.name.text == slot.text) {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                slot.span,
                format!("slot `{}` collides with a state field of the same name", slot.text),
            ));
        }
    }
}

/// Walks a view in the scope of `host` (`None` = a page). Checks the
/// placeholders it contains and every callsite slot block it carries.
fn check_placeholders(
    node: &ViewNode,
    host: Option<&ComponentDecl>,
    model: &Model<'_>,
    diags: &mut Vec<Diagnostic>,
) {
    match node {
        ViewNode::Widget(widget) => {
            check_callsite(widget, host, model, diags);
            check_siblings(&widget.children, diags);
            for child in &widget.children {
                check_placeholders(child, host, model, diags);
            }
        }
        ViewNode::If { arms, els, .. } => {
            // Rule 12 is deliberately per-parent, NOT per-component: `Slot x`
            // in both arms of an `if` is the shape a per-region panel opt-in
            // needs (`if panel { Panel { Slot x } } else { Stack { Slot x } }`),
            // and only one arm is ever live.
            for (_, body) in arms {
                check_siblings(body, diags);
                for child in body {
                    check_placeholders(child, host, model, diags);
                }
            }
            check_siblings(els, diags);
            for child in els {
                check_placeholders(child, host, model, diags);
            }
        }
        ViewNode::For { body, .. } => {
            check_siblings(body, diags);
            for child in body {
                check_placeholders(child, host, model, diags);
            }
        }
        ViewNode::Collection(collection) => {
            check_siblings(&collection.body, diags);
            for child in &collection.body {
                check_placeholders(child, host, model, diags);
            }
        }
        ViewNode::Match { arms, .. } => {
            for arm in arms {
                check_siblings(&arm.body, diags);
                for child in &arm.body {
                    check_placeholders(child, host, model, diags);
                }
            }
        }
        ViewNode::Slot { name, span } => match host {
            // Rule 5.
            Some(component) if !component.slots.iter().any(|s| s.text == name.text) => {
                diags.push(Diagnostic::new(
                    DiagCode::UnknownSlot,
                    *span,
                    format!("`{}` declares no `slot {}`", component.name.text, name.text),
                ));
            }
            Some(_) => {}
            // Rule 6.
            None => diags.push(Diagnostic::new(
                DiagCode::SlotShape,
                *span,
                String::from("`Slot` belongs in a `Component` — a `Page` has no caller to fill it"),
            )),
        },
    }
}

/// Rule 12: two placeholders for one slot under the same parent would both
/// want the same single body at the same time.
fn check_siblings(body: &[ViewNode], diags: &mut Vec<Diagnostic>) {
    for (i, node) in body.iter().enumerate() {
        let ViewNode::Slot { name, span } = node else { continue };
        if body[..i]
            .iter()
            .any(|earlier| matches!(earlier, ViewNode::Slot { name: n, .. } if n.text == name.text))
        {
            diags.push(Diagnostic::new(
                DiagCode::SlotShape,
                *span,
                format!("`Slot {}` appears twice under the same parent", name.text),
            ));
        }
    }
}

/// The callsite side: rules 1–4 and 9–11, plus rule 13 over the bodies.
fn check_callsite(
    widget: &WidgetNode,
    host: Option<&ComponentDecl>,
    model: &Model<'_>,
    diags: &mut Vec<Diagnostic>,
) {
    let callee =
        model.component_by_name.get(widget.name.text.as_str()).map(|&i| model.components[i]);

    let Some(callee) = callee else {
        // Rule 3: a widget has children, not slots.
        if !widget.slot_bodies.is_empty() {
            diags.push(Diagnostic::new(
                DiagCode::SlotShape,
                widget.span,
                format!(
                    "`{}` is a widget — put content in its `{{ … }}` children, not a slot block",
                    widget.name.text
                ),
            ));
        }
        return;
    };

    // Rules 9 + 10: a component reference carries neither children nor
    // modifiers. Both used to be dropped in silence at lowering
    // (`lower/views.rs::lower_component_ref`), so this is a fix, not a
    // restriction — code that "worked" was already losing what it wrote.
    if !widget.children.is_empty() {
        diags.push(Diagnostic::new(
            DiagCode::SlotShape,
            widget.children[0].span(),
            if callee.slots.is_empty() {
                format!(
                    "`{}` takes no content — declare a `slot` on it to accept children",
                    widget.name.text
                )
            } else {
                format!(
                    "`{}` takes content through its slots — name one: `{} {{ … }} {{ {} {{ … }} }}`",
                    widget.name.text, widget.name.text, callee.slots[0].text
                )
            },
        ));
    }
    if let Some(modifier) = widget.modifiers.first() {
        diags.push(Diagnostic::new(
            DiagCode::SlotShape,
            modifier.span,
            format!(
                "`.{}` on a component reference is dropped — apply it inside `{}` or wrap the reference in a `Stack`",
                modifier.name.text, widget.name.text
            ),
        ));
    }

    // Rule 4.
    if !widget.slot_bodies.is_empty() && callee.slots.is_empty() {
        diags.push(Diagnostic::new(
            DiagCode::SlotShape,
            widget.slot_bodies[0].span,
            format!("`{}` declares no slots", widget.name.text),
        ));
        return;
    }

    for (i, binding) in widget.slot_bodies.iter().enumerate() {
        // Rule 1.
        if !callee.slots.iter().any(|s| s.text == binding.name.text) {
            diags.push(Diagnostic::new(
                DiagCode::UnknownSlot,
                binding.name.span,
                format!("`{}` has no slot `{}`", widget.name.text, binding.name.text),
            ));
        }
        // Rule 2.
        if widget.slot_bodies[..i].iter().any(|earlier| earlier.name.text == binding.name.text) {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                binding.name.span,
                format!("slot `{}` is bound twice", binding.name.text),
            ));
        }
        check_body_scope(binding, host, diags);
    }
}

/// Rules 11 + 13 over one slot body: it may not forward the host's own slots,
/// and every `$props.<n>` it names must be a prop of the HOST — the body runs
/// in the caller's frame, never the callee's.
fn check_body_scope(
    binding: &SlotBinding,
    host: Option<&ComponentDecl>,
    diags: &mut Vec<Diagnostic>,
) {
    fn walk(node: &ViewNode, host: Option<&ComponentDecl>, diags: &mut Vec<Diagnostic>) {
        match node {
            ViewNode::Widget(widget) => {
                if let Some(positional) = &widget.positional {
                    props_in_expr(positional, host, diags);
                }
                for (_, value) in &widget.props {
                    props_in_expr(value, host, diags);
                }
                for modifier in &widget.modifiers {
                    for arg in &modifier.args {
                        props_in_expr(&arg.value, host, diags);
                    }
                }
                for child in &widget.children {
                    walk(child, host, diags);
                }
                // A nested component may have slots of its own; those bodies
                // belong to THIS body's scope, which is still the host's.
                for nested in &widget.slot_bodies {
                    for child in &nested.body {
                        walk(child, host, diags);
                    }
                }
            }
            ViewNode::If { arms, els, .. } => {
                for (cond, body) in arms {
                    props_in_expr(cond, host, diags);
                    for child in body {
                        walk(child, host, diags);
                    }
                }
                for child in els {
                    walk(child, host, diags);
                }
            }
            ViewNode::For { iter, body, .. } => {
                props_in_expr(iter, host, diags);
                for child in body {
                    walk(child, host, diags);
                }
            }
            ViewNode::Collection(collection) => {
                props_in_expr(&collection.binding, host, diags);
                for child in &collection.body {
                    walk(child, host, diags);
                }
            }
            ViewNode::Match { scrutinee, arms, .. } => {
                props_in_expr(scrutinee, host, diags);
                for arm in arms {
                    for child in &arm.body {
                        walk(child, host, diags);
                    }
                }
            }
            // Rule 11.
            ViewNode::Slot { name, span } => diags.push(Diagnostic::new(
                DiagCode::SlotShape,
                *span,
                format!(
                    "`Slot {}` cannot be forwarded from inside a slot body \
                     (a body belongs to its caller, not to the component it is passed to)",
                    name.text
                ),
            )),
        }
    }

    for child in &binding.body {
        walk(child, host, diags);
    }
}

/// Rule 13: `$props.<n>` inside a slot body resolves against the HOST.
fn props_in_expr(expr: &Expr, host: Option<&ComponentDecl>, diags: &mut Vec<Diagnostic>) {
    match expr {
        Expr::PropsRef { path, span } => {
            let Some(first) = path.first() else { return };
            let known = host.is_some_and(|h| h.props.iter().any(|p| p.name.text == first.text));
            if !known {
                diags.push(Diagnostic::new(
                    DiagCode::UnknownField,
                    *span,
                    match host {
                        Some(h) => format!(
                            "`$props.{}` in a slot body resolves against `{}` (the caller), \
                             which has no such prop",
                            first.text, h.name.text
                        ),
                        None => format!(
                            "`$props.{}` in a slot body has no enclosing component to resolve against",
                            first.text
                        ),
                    },
                ));
            }
        }
        Expr::Unary { operand, .. } => props_in_expr(operand, host, diags),
        Expr::Binary { lhs, rhs, .. } => {
            props_in_expr(lhs, host, diags);
            props_in_expr(rhs, host, diags);
        }
        Expr::List { items, .. } => {
            for item in items {
                props_in_expr(item, host, diags);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                props_in_expr(&arg.value, host, diags);
            }
        }
        Expr::I18n { args, .. } | Expr::EnumLit { args, .. } => {
            for arg in args {
                props_in_expr(arg, host, diags);
            }
        }
        _ => {}
    }
}
