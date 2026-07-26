// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Modifier checks: does this modifier exist, does it take this many
//! arguments, and is each token argument in that modifier's vocabulary.
//!
//! The last one matters more than it looks. Before RFC-0082 an unknown token
//! (`.fg(oNSurface)`) type-checked cleanly and then resolved to `None` at
//! runtime — the node painted its default and the author got no signal at all.

use crate::ast::{Expr, ModifierCall};
use crate::diag::{DiagCode, Diagnostic};
use crate::registry;
use alloc::{format, vec::Vec};

pub(super) fn check_modifiers(modifiers: &[ModifierCall], diags: &mut Vec<Diagnostic>) {
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
                // Motion modifiers (`.animate`/`.transition`/`.effect`) take a
                // CURATED motion token as the first argument — validate it
                // against the closed set (no free-form animation names).
                if matches!(spec.name, "animate" | "transition" | "effect") {
                    check_motion_token(modifier, diags);
                } else if let Some(vocab) = registry::token_vocabulary(spec.name) {
                    check_token_vocabulary(modifier, vocab, diags);
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
/// vocabulary (RFC-0082). A bare token lowers to a single-segment `Path`;
/// anything else (a string, a number, a state ref) is left alone — the arity
/// check and the runtime's own coercion own those cases.
fn check_token_vocabulary(
    modifier: &ModifierCall,
    vocab: &'static [&'static str],
    diags: &mut Vec<Diagnostic>,
) {
    for arg in &modifier.args {
        let Expr::Path { segments, .. } = &arg.value else { continue };
        let [segment] = segments.as_slice() else { continue };
        if vocab.contains(&segment.text.as_str()) {
            continue;
        }
        // Near-miss first: a casing slip is the common failure and naming it
        // is far more useful than printing 36 color roles.
        let hint = vocab
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(&segment.text))
            .map_or_else(
                || format!("expected one of: {}", vocab.join(", ")),
                |candidate| format!("did you mean `{candidate}`?"),
            );
        diags.push(Diagnostic::new(
            DiagCode::UnknownName,
            arg.value.span(),
            format!("`.{}` has no token `{}` — {hint}", modifier.name.text, segment.text),
        ));
    }
}
