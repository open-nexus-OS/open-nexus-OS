// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: App-export mediation (TASK-0081 decision C2, manifest v2.2):
//! resolves a consumer's request for an exported ability against the
//! build-time export table — BOTH sides checked fail-closed (the exporter
//! must declare the export, the consumer must hold the app-owned
//! permission `app.<bundle>.<CAP>` in its manifest caps). This is the pure
//! decision core ("mediated"); the channel half ("then direct": ensure the
//! exporter runs, mint the endpoint pair, hand out the caps) rides on the
//! OS loop next to `spawn_app` — no broker ever sits in the data path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0081)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: grant-matrix unit tests below

use crate::caps;

/// Why a resolve was refused. Stable, matchable — the wire maps these to
/// status codes, never strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediationError {
    /// No installed app exports this ability.
    UnknownAbility,
    /// The consumer's manifest does not declare the export's permission.
    ConsumerNotGranted,
}

/// A successfully mediated export: who serves it, under which permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedExport {
    pub exporter: &'static str,
    pub ability: &'static str,
    pub permission: &'static str,
}

/// Resolves `ability` for a consumer holding `consumer_caps`
/// (its manifest-declared capability list — the CALLER binds identity, the
/// OS loop passes `caps::required_caps(<verified sender app>)`).
///
/// Fail-closed on both sides:
/// - nobody exports the ability ⇒ [`MediationError::UnknownAbility`];
/// - the consumer lacks the app-owned permission ⇒
///   [`MediationError::ConsumerNotGranted`].
///
/// # Errors
/// See above — every refusal is a stable, distinguishable error.
pub fn resolve_export(
    consumer_caps: &[&str],
    ability: &str,
) -> Result<ResolvedExport, MediationError> {
    let (exporter, exported_ability, permission) =
        caps::find_export(ability).ok_or(MediationError::UnknownAbility)?;
    if !consumer_caps.iter().any(|cap| *cap == permission) {
        return Err(MediationError::ConsumerNotGranted);
    }
    Ok(ResolvedExport { exporter, ability: exported_ability, permission })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_matrix_is_fail_closed_on_both_sides() {
        // Granted consumer resolves the chat reference export.
        let granted = ["nexus.permission.WINDOW", "app.chat.SEND"];
        let resolved = resolve_export(&granted, "chat.Send").expect("resolves");
        assert_eq!(resolved.exporter, "chat");
        assert_eq!(resolved.permission, "app.chat.SEND");

        // Same consumer, ability whose permission it does NOT hold.
        assert_eq!(
            resolve_export(&granted, "chat.Receive"),
            Err(MediationError::ConsumerNotGranted)
        );
        // Ungranted consumer is refused even knowing the ability exists.
        assert_eq!(
            resolve_export(&["nexus.permission.WINDOW"], "chat.Send"),
            Err(MediationError::ConsumerNotGranted)
        );
        // Nobody exports it: unknown BEFORE any grant question.
        assert_eq!(resolve_export(&granted, "chat.Delete"), Err(MediationError::UnknownAbility));
        // Empty caps: nothing resolves.
        assert_eq!(resolve_export(&[], "chat.Send"), Err(MediationError::ConsumerNotGranted));
    }
}
