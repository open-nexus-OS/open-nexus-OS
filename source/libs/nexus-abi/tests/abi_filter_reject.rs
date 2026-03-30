//! CONTEXT: Security-negative tests for ABI syscall filter profile handling.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0019 required `test_reject_*` host proofs

use nexus_abi::abi_filter::{
    decode_profile_v1, encode_profile_v1, ingest_distributed_profile_v1,
    ingest_distributed_profile_v1_typed, AbiFilterError, AuthorityServiceId, RuleAction,
    SenderServiceId, SubjectServiceId, SyscallClass, MAX_PROFILE_BYTES, MAX_RULES,
    MAX_STATEFS_PUT_BYTES, PROFILE_MAGIC0, PROFILE_MAGIC1, PROFILE_VERSION,
};

#[test]
fn test_reject_unbounded_profile() {
    let oversized = vec![0u8; MAX_PROFILE_BYTES + 1];
    let err = decode_profile_v1(&oversized).unwrap_err();
    assert_eq!(err, AbiFilterError::OversizedProfile);
}

#[test]
fn test_reject_unauthenticated_profile_distribution() {
    let subject = nexus_abi::service_id_from_name(b"selftest-client");
    let authority = nexus_abi::service_id_from_name(b"policyd");
    let forged_sender = nexus_abi::service_id_from_name(b"bundlemgrd");
    let mut buf = [0u8; MAX_PROFILE_BYTES];
    let n =
        encode_profile_v1(subject, Some(b"/state/app/selftest/"), Some(1024), &mut buf).unwrap();

    let err = ingest_distributed_profile_v1(&buf[..n], forged_sender, authority, subject).unwrap_err();
    assert_eq!(err, AbiFilterError::UnauthenticatedProfileDistribution);
}

#[test]
fn test_reject_subject_spoofed_profile_identity() {
    let authority = nexus_abi::service_id_from_name(b"policyd");
    let sender = authority;
    let payload_subject = nexus_abi::service_id_from_name(b"demo.testsvc");
    let expected_subject = nexus_abi::service_id_from_name(b"selftest-client");
    let mut buf = [0u8; MAX_PROFILE_BYTES];
    let n = encode_profile_v1(payload_subject, Some(b"/state/app/selftest/"), None, &mut buf).unwrap();

    let err = ingest_distributed_profile_v1(&buf[..n], sender, authority, expected_subject).unwrap_err();
    assert_eq!(err, AbiFilterError::SubjectIdentityMismatch);
}

#[test]
fn test_reject_profile_rule_count_overflow() {
    let mut malformed = [0u8; 12];
    malformed[0] = PROFILE_MAGIC0;
    malformed[1] = PROFILE_MAGIC1;
    malformed[2] = PROFILE_VERSION;
    malformed[3] = (MAX_RULES as u8).saturating_add(1);
    let err = decode_profile_v1(&malformed).unwrap_err();
    assert_eq!(err, AbiFilterError::RuleCountOverflow);
}

#[test]
fn test_reject_first_match_precedence_conflict_is_deterministic() {
    let subject = nexus_abi::service_id_from_name(b"selftest-client");
    let prefix = b"/state/app/";
    let mut profile = Vec::new();
    profile.push(PROFILE_MAGIC0);
    profile.push(PROFILE_MAGIC1);
    profile.push(PROFILE_VERSION);
    profile.push(2); // rule_count
    profile.extend_from_slice(&subject.to_le_bytes());

    // Rule 0: deny first
    profile.push(SyscallClass::StatefsPut as u8);
    profile.push(RuleAction::Deny as u8);
    profile.push(prefix.len() as u8);
    profile.push(0);
    profile.extend_from_slice(&0u16.to_le_bytes());
    profile.extend_from_slice(&0u16.to_le_bytes());
    profile.extend_from_slice(prefix);

    // Rule 1: allow second for the same prefix (must not win)
    profile.push(SyscallClass::StatefsPut as u8);
    profile.push(RuleAction::Allow as u8);
    profile.push(prefix.len() as u8);
    profile.push(0);
    profile.extend_from_slice(&0u16.to_le_bytes());
    profile.extend_from_slice(&0u16.to_le_bytes());
    profile.extend_from_slice(prefix);

    let parsed = decode_profile_v1(&profile).unwrap();
    assert_eq!(
        parsed.check_statefs_put(b"/state/app/selftest/token", 8),
        RuleAction::Deny
    );
}

#[test]
fn test_reject_trailing_profile_bytes_as_malformed() {
    let subject = nexus_abi::service_id_from_name(b"selftest-client");
    let mut buf = [0u8; MAX_PROFILE_BYTES];
    let n = encode_profile_v1(subject, Some(b"/state/app/selftest/"), Some(1024), &mut buf).unwrap();
    let mut malformed = Vec::from(&buf[..n]);
    malformed.push(0xaa); // trailing byte must be rejected deterministically
    let err = decode_profile_v1(&malformed).unwrap_err();
    assert_eq!(err, AbiFilterError::MalformedProfile);
}

#[test]
fn test_reject_statefs_put_oversized_payload_fail_closed() {
    let subject = nexus_abi::service_id_from_name(b"selftest-client");
    let mut buf = [0u8; MAX_PROFILE_BYTES];
    let n = encode_profile_v1(subject, Some(b"/state/app/selftest/"), None, &mut buf).unwrap();
    let profile = decode_profile_v1(&buf[..n]).unwrap();
    assert_eq!(
        profile.check_statefs_put(b"/state/app/selftest/token", MAX_STATEFS_PUT_BYTES + 1),
        RuleAction::Deny
    );
}

#[test]
fn test_reject_typed_distribution_subject_mismatch() {
    let authority = nexus_abi::service_id_from_name(b"policyd");
    let mut buf = [0u8; MAX_PROFILE_BYTES];
    let payload_subject = nexus_abi::service_id_from_name(b"demo.testsvc");
    let expected_subject = nexus_abi::service_id_from_name(b"selftest-client");
    let n = encode_profile_v1(payload_subject, Some(b"/state/app/selftest/"), None, &mut buf).unwrap();
    let err = ingest_distributed_profile_v1_typed(
        &buf[..n],
        SenderServiceId::new(authority),
        AuthorityServiceId::new(authority),
        SubjectServiceId::new(expected_subject),
    )
    .unwrap_err();
    assert_eq!(err, AbiFilterError::SubjectIdentityMismatch);
}
