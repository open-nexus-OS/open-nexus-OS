// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! One blur per stack (RFC-0116): only the surface that sits on the wallpaper blurs. A node with
//! a blur material (`panel`, `window`, `dock`, `thin`, `ultraThin` — or the `Panel` widget, which
//! is a panel unless its material says otherwise) inside another one is NX0414.
//!
//! The walk sees one page or component body: a component instantiated inside a blur is not
//! followed into (its body is checked on its own), so the rule is exact within a body and the
//! compositor's one-region-per-stack is the backstop across bodies.

use super::Model;
use crate::ast::{Expr, ViewNode, WidgetNode};
use crate::diag::{DiagCode, Diagnostic};
use crate::registry;
use alloc::{format, vec::Vec};

pub(super) fn run(model: &Model<'_>, diags: &mut Vec<Diagnostic>) {
    for page in &model.pages {
        walk(&page.view, false, diags);
    }
    for component in &model.components {
        walk(&component.view, false, diags);
    }
}

/// The material a widget's `.material(…)` names, if it names one by token.
fn material_of(widget: &WidgetNode) -> Option<&str> {
    let call = widget.modifiers.iter().find(|m| m.name.text == "material")?;
    match &call.args.first()?.value {
        Expr::Path { segments, .. } if segments.len() == 1 => Some(segments[0].text.as_str()),
        _ => None,
    }
}

/// Whether this widget blurs what lies behind it.
fn blurs(widget: &WidgetNode) -> bool {
    match material_of(widget) {
        Some(material) => registry::BLUR_MATERIALS.contains(&material),
        None => widget.name.text == "Panel",
    }
}

fn walk(node: &ViewNode, inside_blur: bool, diags: &mut Vec<Diagnostic>) {
    match node {
        ViewNode::Widget(widget) => {
            let blur = blurs(widget);
            if blur && inside_blur {
                diags.push(Diagnostic::new(
                    DiagCode::NestedBlur,
                    widget.span,
                    format!(
                        "`{}` blurs inside a blur — a layer on a blur surface is \
                         `.material(card)` (no blur) or opaque (`pane`, `overlay`)",
                        widget.name.text
                    ),
                ));
            }
            let inside = inside_blur || blur;
            for child in &widget.children {
                walk(child, inside, diags);
            }
            for binding in &widget.slot_bodies {
                for child in &binding.body {
                    walk(child, inside, diags);
                }
            }
        }
        ViewNode::If { arms, els, .. } => {
            for (_, body) in arms {
                for child in body {
                    walk(child, inside_blur, diags);
                }
            }
            for child in els {
                walk(child, inside_blur, diags);
            }
        }
        ViewNode::For { body, .. } => {
            for child in body {
                walk(child, inside_blur, diags);
            }
        }
        ViewNode::Collection(collection) => {
            for child in &collection.body {
                walk(child, inside_blur, diags);
            }
        }
        ViewNode::Match { arms, .. } => {
            for arm in arms {
                for child in &arm.body {
                    walk(child, inside_blur, diags);
                }
            }
        }
        ViewNode::Slot { .. } => {}
    }
}
