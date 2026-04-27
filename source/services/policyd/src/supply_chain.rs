//! CONTEXT: Supply-chain sign-policy decision mapping for install authority path
//! OWNERS: @security @runtime
//! STATUS: Functional
//! API_STABILITY: Internal module
//! TEST_COVERAGE: 3 unit tests
//!   - maps_allowlist_reason_labels
//!   - decision_uses_authority_response
//!   - decision_maps_authority_errors
#![forbid(unsafe_code)]

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignPolicyDecision {
    Allow,
    Deny { label: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "sign-policy authority query failures must be handled"]
pub enum SignPolicyError {
    QueryFailed,
}

pub fn evaluate_allowlist(allowed: bool, reason: &str) -> SignPolicyDecision {
    if allowed {
        return SignPolicyDecision::Allow;
    }
    let label = match reason {
        "publisher_unknown" => "policy.publisher_unknown",
        "key_unknown" => "policy.key_unknown",
        "alg_unsupported" => "policy.alg_unsupported",
        "disabled" => "policy.disabled",
        _ => "policy.denied",
    };
    SignPolicyDecision::Deny { label }
}

pub fn decide_from_authority<F, E, R>(
    publisher: &str,
    alg: &str,
    pubkey: &[u8],
    mut query_authority: F,
) -> Result<SignPolicyDecision, SignPolicyError>
where
    F: FnMut(&str, &str, &[u8]) -> Result<(bool, R), E>,
    R: AsRef<str>,
{
    let (allowed, reason) =
        query_authority(publisher, alg, pubkey).map_err(|_| SignPolicyError::QueryFailed)?;
    Ok(evaluate_allowlist(allowed, reason.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::{decide_from_authority, evaluate_allowlist, SignPolicyDecision, SignPolicyError};

    #[test]
    fn maps_allowlist_reason_labels() {
        assert_eq!(evaluate_allowlist(true, "allow"), SignPolicyDecision::Allow);
        assert_eq!(
            evaluate_allowlist(false, "publisher_unknown"),
            SignPolicyDecision::Deny {
                label: "policy.publisher_unknown"
            }
        );
        assert_eq!(
            evaluate_allowlist(false, "key_unknown"),
            SignPolicyDecision::Deny {
                label: "policy.key_unknown"
            }
        );
        assert_eq!(
            evaluate_allowlist(false, "alg_unsupported"),
            SignPolicyDecision::Deny {
                label: "policy.alg_unsupported"
            }
        );
        assert_eq!(
            evaluate_allowlist(false, "disabled"),
            SignPolicyDecision::Deny {
                label: "policy.disabled"
            }
        );
    }

    #[test]
    fn decision_uses_authority_response() {
        let allow = decide_from_authority("pub", "ed25519", b"k", |_p, _a, _k| {
            Ok::<(bool, String), ()>((true, "allow".to_string()))
        })
        .expect("allow query");
        assert_eq!(allow, SignPolicyDecision::Allow);

        let deny = decide_from_authority("pub", "ed25519", b"k", |_p, _a, _k| {
            Ok::<(bool, String), ()>((false, "key_unknown".to_string()))
        })
        .expect("deny query");
        assert_eq!(
            deny,
            SignPolicyDecision::Deny {
                label: "policy.key_unknown"
            }
        );
    }

    #[test]
    fn decision_maps_authority_errors() {
        let err = decide_from_authority("pub", "ed25519", b"k", |_p, _a, _k| {
            Err::<(bool, String), _>("backend down")
        })
        .expect_err("query error");
        assert_eq!(err, SignPolicyError::QueryFailed);
    }
}
