// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Modifier checks: does this modifier exist (and is it not retired), does it take this many
//! arguments, is each token argument in that modifier's vocabulary, and is each spacing on the
//! 4-pt grid.
//!
//! The vocabulary check matters more than it looks. Before RFC-0082 an unknown token
//! (`.fg(oNSurface)`) type-checked cleanly and then resolved to `None` at runtime — the node
//! painted its default and the author got no signal at all. Since RFC-0116 a name the token set
//! retired names its replacement instead of listing the vocabulary.

use crate::ast::{Expr, ModifierCall};
use crate::diag::{DiagCode, Diagnostic};
use crate::registry;
use alloc::{format, string::String, vec::Vec};

pub(super) fn check_modifiers(modifiers: &[ModifierCall], diags: &mut Vec<Diagnostic>) {
    for modifier in modifiers {
        if let Some(hint) = registry::retired_modifier(&modifier.name.text) {
            diags.push(Diagnostic::new(
                DiagCode::UnknownModifier,
                modifier.name.span,
                format!("`.{}` was retired by RFC-0116 — {hint}", modifier.name.text),
            ));
            continue;
        }
        match registry::modifier_spec(&modifier.name.text) {
            None => diags.push(Diagnostic::new(
                DiagCode::UnknownModifier,
                modifier.name.span,
                format!("unknown modifier `.{}`", modifier.name.text),
            )),
            Some((_, spec)) => {
                // An `OptToken` arg may be omitted: arity is `required..=all`.
                let required =
                    spec.args.iter().filter(|a| !matches!(a, registry::ModArg::OptToken)).count();
                if modifier.args.len() < required || modifier.args.len() > spec.args.len() {
                    let takes = if required == spec.args.len() {
                        format!("{required}")
                    } else {
                        format!("{required}..={}", spec.args.len())
                    };
                    diags.push(Diagnostic::new(
                        DiagCode::WrongArity,
                        modifier.span,
                        format!(
                            "`.{}` takes {takes} argument(s), got {}",
                            spec.name,
                            modifier.args.len()
                        ),
                    ));
                }
                // Motion modifiers (`.animate`/`.transition`/`.effect`) take a
                // CURATED motion token as the first argument — validate it
                // against the closed set (no free-form animation names).
                if matches!(spec.name, "animate" | "transition" | "effect") {
                    check_motion_token(modifier, diags);
                } else {
                    check_token_vocabulary(modifier, diags);
                }
                if registry::SPACING_MODIFIERS.contains(&spec.name) {
                    check_spacing(modifier, diags);
                }
            }
        }
    }
}

/// The first argument of a motion modifier must be one of the curated
/// [`registry::MOTION_TOKENS`]. A bare token lowers to a single-segment
/// `Path`/`DeviceRef`-style identifier — read that name and reject anything
/// outside the set (an unknown token is a compile error, not a silent no-op).
fn check_motion_token(modifier: &ModifierCall, diags: &mut Vec<Diagnostic>) {
    let Some(first) = modifier.args.first() else { return };
    let name = match &first.value {
        Expr::Path { segments, .. } if segments.len() == 1 => Some(segments[0].text.as_str()),
        _ => None,
    };
    match name {
        Some(name) if registry::is_motion_token(name) => {}
        _ => diags.push(Diagnostic::new(
            DiagCode::UnknownName,
            first.value.span(),
            format!(
                "`.{}` expects a motion token ({}), not `{}`",
                modifier.name.text,
                registry::MOTION_TOKENS.join(", "),
                name.unwrap_or("a value")
            ),
        )),
    }
}

/// Every argument of a closed-vocabulary modifier must name a token in that
/// vocabulary (RFC-0082, RFC-0116). A bare token lowers to a single-segment `Path`;
/// a string literal naming a colour role is checked too (a role may come from data, so the
/// runtime also accepts a string). Anything else (a number, a state ref) is left alone — the
/// arity check, the spacing check and the runtime's own coercion own those cases.
fn check_token_vocabulary(modifier: &ModifierCall, diags: &mut Vec<Diagnostic>) {
    let name = modifier.name.text.as_str();
    for (index, arg) in modifier.args.iter().enumerate() {
        let Some(vocab) = registry::token_vocabulary_at(name, index) else { continue };
        let token = match &arg.value {
            Expr::Path { segments, .. } => match segments.as_slice() {
                [segment] => segment.text.as_str(),
                _ => continue,
            },
            Expr::Str { value, .. } if registry::COLOR_MODIFIERS.contains(&name) => value.as_str(),
            _ => continue,
        };
        if vocab.contains(&token) {
            continue;
        }
        let hint = match registry::retired_token(name, token) {
            Some(replacement) => format!("retired by RFC-0116, use {replacement}"),
            // Near-miss next: a casing slip is the common failure and naming it is far more
            // useful than printing 35 colour roles.
            None => {
                vocab.iter().find(|candidate| candidate.eq_ignore_ascii_case(token)).map_or_else(
                    || format!("expected one of: {}", vocab.join(", ")),
                    |candidate| format!("did you mean `{candidate}`?"),
                )
            }
        };
        diags.push(Diagnostic::new(
            DiagCode::UnknownName,
            arg.value.span(),
            format!("`.{name}` has no token `{token}` — {hint}"),
        ));
    }
}

/// A spacing (`.padding`, `.gap`, `.rowGap`, `.hitSlop`, …) is whole px on the 4-pt grid
/// (RFC-0116) or a spacing name; anything else is a compile error — `.gap(1.5)` used to compile
/// and lay out 0, and `.padding(7)` a 28 px the grid does not have.
fn check_spacing(modifier: &ModifierCall, diags: &mut Vec<Diagnostic>) {
    for arg in &modifier.args {
        let off_grid = match &arg.value {
            Expr::Int { value, .. } => !registry::SPACING_GRID.contains(value),
            Expr::Fx { .. } => true,
            _ => false,
        };
        if off_grid {
            let grid: Vec<String> = registry::SPACING_GRID.iter().map(|v| format!("{v}")).collect();
            diags.push(Diagnostic::new(
                DiagCode::UnknownName,
                arg.value.span(),
                format!(
                    "`.{}` is off the 4-pt grid — use px {} or {}",
                    modifier.name.text,
                    grid.join(", "),
                    registry::SPACING_TOKENS.join(", ")
                ),
            ));
        }
    }
}
