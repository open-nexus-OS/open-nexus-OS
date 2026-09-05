// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 required negative tests for the v2 matcher/codec
//! (`cargo test -p nexus-abi -- v2_reject`). Names are the contract.
//! `test_reject_unauthenticated_mode_switch` lives with policyd (the
//! authority that authenticates `OP_SET_ABI_MODE`, TASK-0028 P3) and
//! `test_learn_roundtrip` with the generator (TASK-0028 P2).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0028 P1 required `test_reject_*` host proofs

mod v2_reject {
    use nexus_abi::abi_filter::{
        decode_profile, decode_profile_v1, decode_profile_v2, encode_profile_v1, encode_profile_v2,
        AbiFilterError, AbiLimits, AbiProfile, AbiRule, AddrClass, PortRange, RuleAction,
        MAX_PATH_PREFIX_BYTES, MAX_PROFILE_BYTES, MAX_RULES, MAX_STATEFS_PATH_BYTES,
        PROFILE_MAGIC0, PROFILE_MAGIC1, PROFILE_VERSION, PROFILE_VERSION_V2,
    };

    const SUBJECT: u64 = 0x1122_3344_5566_7788;

    fn ports(list: &[(u16, u16)]) -> Vec<PortRange> {
        list.iter().map(|&(min, max)| PortRange { min, max }).collect()
    }

    fn profile_with(rules: &[AbiRule]) -> AbiProfile {
        let mut p = AbiProfile::empty(SUBJECT).with_epoch(3);
        for r in rules {
            p.push_rule(*r).unwrap();
        }
        p
    }

    fn v2_bytes(p: &AbiProfile) -> Vec<u8> {
        let mut buf = [0u8; MAX_PROFILE_BYTES];
        let n = encode_profile_v2(p, &mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn test_reject_first_match_shadowing() {
        let allow = AbiRule::statefs(RuleAction::Allow, b"/state/app/").unwrap();
        let deny = AbiRule::statefs(RuleAction::Deny, b"/state/app/secrets/").unwrap();
        // Broad allow authored FIRST no longer shadows the narrower deny.
        let p = profile_with(&[allow, deny]);
        assert_eq!(p.check_statefs_put(b"/state/app/secrets/key", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/token", 8), RuleAction::Allow);
        // Order-independent: same verdicts with the rules swapped.
        let p = profile_with(&[deny, allow]);
        assert_eq!(p.check_statefs_put(b"/state/app/secrets/key", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/token", 8), RuleAction::Allow);
        // Equal specificity: deny beats allow regardless of order.
        let deny_same = AbiRule::statefs(RuleAction::Deny, b"/state/app/").unwrap();
        assert_eq!(
            profile_with(&[allow, deny_same]).check_statefs_put(b"/state/app/token", 8),
            RuleAction::Deny
        );
        assert_eq!(
            profile_with(&[deny_same, allow]).check_statefs_put(b"/state/app/token", 8),
            RuleAction::Deny
        );
        // Narrower allow under a broad deny wins (most specific rule).
        let broad_deny = AbiRule::statefs(RuleAction::Deny, b"/state/").unwrap();
        let narrow_allow = AbiRule::statefs(RuleAction::Allow, b"/state/app/selftest/").unwrap();
        let p = profile_with(&[broad_deny, narrow_allow]);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/x", 8), RuleAction::Allow);
        assert_eq!(p.check_statefs_put(b"/state/app/other", 8), RuleAction::Deny);
    }

    #[test]
    fn test_reject_argument_injection() {
        let allow = AbiRule::statefs(RuleAction::Allow, b"/state/app/selftest/").unwrap();
        let p = profile_with(&[allow]);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/token", 8), RuleAction::Allow);
        // Parent-segment escape under an allowed prefix.
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/../secret", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/..", 8), RuleAction::Deny);
        // Dot segment, empty segment, trailing slash, embedded NUL, relative path.
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/./t", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest//t", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/t/", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/t\0x", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"state/app/selftest/t", 8), RuleAction::Deny);
        // Over-long path is denied, never truncated into a match.
        let mut long = b"/state/app/selftest/".to_vec();
        long.resize(MAX_STATEFS_PATH_BYTES + 1, b'a');
        assert_eq!(p.check_statefs_put(&long, 8), RuleAction::Deny);
        // A literal-looking prefix is NOT a pattern: `*` matches only `*`.
        let star = AbiRule::statefs(RuleAction::Allow, b"/state/app/*").unwrap();
        let p = profile_with(&[star]);
        assert_eq!(p.check_statefs_put(b"/state/app/x", 8), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/app/*x", 8), RuleAction::Allow);
    }

    #[test]
    fn test_reject_regex_dos() {
        // Matching is bounded literal comparison: a pathological
        // 24-rule × 128-byte case terminates in one pass and the
        // near-miss prefixes never match by "pattern" semantics.
        let mut rules = Vec::new();
        for i in 0..MAX_RULES {
            let mut prefix = b"/state/(a|aa|aaa)+".to_vec();
            prefix.push(b'a' + (i % 26) as u8);
            rules.push(AbiRule::statefs(RuleAction::Allow, &prefix).unwrap());
        }
        let p = profile_with(&rules);
        let mut path = b"/state/".to_vec();
        path.resize(MAX_STATEFS_PATH_BYTES, b'a');
        assert_eq!(p.check_statefs_put(&path, 8), RuleAction::Deny);
        // The exact literal (metacharacters included) still matches literally.
        assert_eq!(p.check_statefs_put(b"/state/(a|aa|aaa)+a/x", 8), RuleAction::Allow);
        // Codec bound: the prefix length is capped, never patterned.
        let too_long = vec![b'a'; MAX_PATH_PREFIX_BYTES + 1];
        assert_eq!(
            AbiRule::statefs(RuleAction::Allow, &too_long).unwrap_err(),
            AbiFilterError::PathPrefixOverflow
        );
    }

    #[test]
    fn test_reject_stale_profile_epoch() {
        let cached = AbiProfile::empty(SUBJECT).with_epoch(3);
        let older = decode_profile(&v2_bytes(&AbiProfile::empty(SUBJECT).with_epoch(2))).unwrap();
        assert_eq!(
            older.check_replaces_epoch(cached.epoch()).unwrap_err(),
            AbiFilterError::StaleEpoch
        );
        let same = decode_profile(&v2_bytes(&AbiProfile::empty(SUBJECT).with_epoch(3))).unwrap();
        assert!(same.check_replaces_epoch(cached.epoch()).is_ok());
        let newer = decode_profile(&v2_bytes(&AbiProfile::empty(SUBJECT).with_epoch(4))).unwrap();
        assert!(newer.check_replaces_epoch(cached.epoch()).is_ok());
        // A v1 profile carries epoch 0 and is stale against any cached v2 epoch ≥ 1.
        let mut buf = [0u8; MAX_PROFILE_BYTES];
        let n = encode_profile_v1(SUBJECT, Some(b"/state/app/"), None, &mut buf).unwrap();
        let v1 = decode_profile(&buf[..n]).unwrap();
        assert_eq!(v1.epoch(), 0);
        assert_eq!(v1.check_replaces_epoch(1).unwrap_err(), AbiFilterError::StaleEpoch);
    }

    #[test]
    fn test_reject_unknown_class_fails_closed() {
        let p = profile_with(&[AbiRule::statefs(RuleAction::Allow, b"/state/").unwrap()]);
        let mut bytes = v2_bytes(&p);
        bytes[20] = 9; // class byte of rule 0
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::InvalidSyscallClass);
        // v1 never carried net.connect: class 3 in a v1 frame is unknown there too.
        let mut v1 = vec![PROFILE_MAGIC0, PROFILE_MAGIC1, PROFILE_VERSION, 1];
        v1.extend_from_slice(&SUBJECT.to_le_bytes());
        v1.extend_from_slice(&[3, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(decode_profile_v1(&v1).unwrap_err(), AbiFilterError::InvalidSyscallClass);
        // Unknown action / address class / reserved bytes / flags fail closed.
        let mut bytes = v2_bytes(&p);
        bytes[21] = 7;
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::InvalidRuleAction);
        let bind = profile_with(&[AbiRule::net_bind(
            RuleAction::Allow,
            AddrClass::Loopback,
            &ports(&[(1024, 65535)]),
        )
        .unwrap()]);
        let mut bytes = v2_bytes(&bind);
        bytes[23] = 5;
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::InvalidAddrClass);
        let mut bytes = v2_bytes(&p);
        bytes[26] = 1; // reserved u16
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::MalformedProfile);
        let mut bytes = v2_bytes(&p);
        bytes[17] = 0x80; // unknown header flag
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::MalformedProfile);
        // Unknown version byte.
        let mut bytes = v2_bytes(&p);
        bytes[2] = 3;
        assert_eq!(decode_profile(&bytes).unwrap_err(), AbiFilterError::MalformedProfile);
        // Trailing byte.
        let mut bytes = v2_bytes(&p);
        bytes.push(0);
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::MalformedProfile);
        // Cross-class field leakage (a statefs rule carrying a CIDR).
        let mut bytes = v2_bytes(&p);
        bytes[28] = 10;
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::MalformedProfile);
    }

    #[test]
    fn test_reject_oversized_profile_v2() {
        let big = vec![0u8; MAX_PROFILE_BYTES + 1];
        assert_eq!(decode_profile(&big).unwrap_err(), AbiFilterError::OversizedProfile);
        assert_eq!(decode_profile_v2(&big).unwrap_err(), AbiFilterError::OversizedProfile);
        // MAX_RULES × maximal prefixes cannot be encoded — never truncated.
        let long = vec![b'/'; MAX_PATH_PREFIX_BYTES];
        let mut p = AbiProfile::empty(SUBJECT);
        for _ in 0..MAX_RULES {
            p.push_rule(AbiRule::statefs(RuleAction::Allow, &long).unwrap()).unwrap();
        }
        let mut buf = [0u8; 4096];
        assert_eq!(encode_profile_v2(&p, &mut buf).unwrap_err(), AbiFilterError::OversizedProfile);
        // Rule count overflow at the builder.
        let mut p = AbiProfile::empty(SUBJECT);
        for _ in 0..MAX_RULES {
            p.push_rule(AbiRule::statefs(RuleAction::Allow, b"/s/").unwrap()).unwrap();
        }
        assert_eq!(
            p.push_rule(AbiRule::statefs(RuleAction::Allow, b"/s/").unwrap()).unwrap_err(),
            AbiFilterError::RuleCountOverflow
        );
        // Header claiming more rules than the bound.
        let mut bytes = v2_bytes(&AbiProfile::empty(SUBJECT));
        bytes[3] = (MAX_RULES + 1) as u8;
        assert_eq!(decode_profile_v2(&bytes).unwrap_err(), AbiFilterError::RuleCountOverflow);
        // Port-range and CIDR bounds at the builder.
        assert_eq!(
            AbiRule::net_bind(RuleAction::Allow, AddrClass::Any, &ports(&[(1, 2); 5])).unwrap_err(),
            AbiFilterError::PortRangeOverflow
        );
        assert_eq!(
            AbiRule::net_bind(RuleAction::Allow, AddrClass::Any, &ports(&[(20, 10)])).unwrap_err(),
            AbiFilterError::PortRangeOverflow
        );
        assert_eq!(
            AbiRule::net_connect(RuleAction::Allow, [10, 0, 2, 1], 24, &ports(&[(80, 80)]))
                .unwrap_err(),
            AbiFilterError::InvalidCidr
        );
        assert_eq!(
            AbiRule::net_connect(RuleAction::Allow, [10, 0, 2, 0], 33, &ports(&[(80, 80)]))
                .unwrap_err(),
            AbiFilterError::InvalidCidr
        );
    }

    #[test]
    fn v2_roundtrip_and_v1_transcode() {
        let statefs = AbiRule::statefs(RuleAction::Allow, b"/state/app/selftest/")
            .unwrap()
            .with_max_payload(2048);
        let bind = AbiRule::net_bind(
            RuleAction::Allow,
            AddrClass::Loopback,
            &ports(&[(1024, 65535), (443, 443)]),
        )
        .unwrap();
        let connect = AbiRule::net_connect(
            RuleAction::Allow,
            [10, 0, 2, 0],
            24,
            &ports(&[(80, 80), (443, 443), (1024, 2048)]),
        )
        .unwrap();
        let p = profile_with(&[statefs, bind, connect])
            .with_limits(AbiLimits { max_payload: 4096, deadline_ms: 2000 });
        let bytes = v2_bytes(&p);
        assert_eq!(bytes[2], PROFILE_VERSION_V2);
        let back = decode_profile(&bytes).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.epoch(), 3);
        assert_eq!(back.limits(), Some(AbiLimits { max_payload: 4096, deadline_ms: 2000 }));
        // Bytes are deterministic (same profile ⇒ same frame).
        assert_eq!(v2_bytes(&back), bytes);
        // v1 transcodes per RFC-0091 §1: allow prefix + loopback-only bind ≥ min.
        let mut buf = [0u8; MAX_PROFILE_BYTES];
        let n = encode_profile_v1(SUBJECT, Some(b"/state/app/selftest/"), Some(1024), &mut buf)
            .unwrap();
        let v1 = decode_profile(&buf[..n]).unwrap();
        assert_eq!(v1.rule_count(), 2);
        assert_eq!(v1.check_statefs_put(b"/state/app/selftest/token", 16), RuleAction::Allow);
        assert_eq!(v1.check_statefs_put(b"/state/forbidden", 16), RuleAction::Deny);
        assert_eq!(v1.check_net_bind(80, AddrClass::Loopback), RuleAction::Deny);
        assert_eq!(v1.check_net_bind(1024, AddrClass::Loopback), RuleAction::Allow);
        assert_eq!(v1.check_net_bind(1024, AddrClass::Any), RuleAction::Deny);
        // A v1 frame re-encodes as v2 and decodes back equal.
        assert_eq!(decode_profile(&v2_bytes(&v1)).unwrap(), v1);
    }

    #[test]
    fn v2_net_bind_precedence_and_address_class() {
        let broad_any =
            AbiRule::net_bind(RuleAction::Allow, AddrClass::Any, &ports(&[(1024, 65535)])).unwrap();
        let narrow_deny =
            AbiRule::net_bind(RuleAction::Deny, AddrClass::Any, &ports(&[(8080, 8080)])).unwrap();
        let p = profile_with(&[broad_any, narrow_deny]);
        assert_eq!(p.check_net_bind(8080, AddrClass::Loopback), RuleAction::Deny);
        assert_eq!(p.check_net_bind(8081, AddrClass::Any), RuleAction::Allow);
        assert_eq!(p.check_net_bind(80, AddrClass::Loopback), RuleAction::Deny);
        // A loopback-only allow never admits an any-interface bind.
        let lo = AbiRule::net_bind(RuleAction::Allow, AddrClass::Loopback, &ports(&[(9000, 9000)]))
            .unwrap();
        let p = profile_with(&[lo]);
        assert_eq!(p.check_net_bind(9000, AddrClass::Loopback), RuleAction::Allow);
        assert_eq!(p.check_net_bind(9000, AddrClass::Any), RuleAction::Deny);
        // Same width: loopback rule is more specific than the any rule.
        let any_deny =
            AbiRule::net_bind(RuleAction::Deny, AddrClass::Any, &ports(&[(9000, 9000)])).unwrap();
        let p = profile_with(&[any_deny, lo]);
        assert_eq!(p.check_net_bind(9000, AddrClass::Loopback), RuleAction::Allow);
        assert_eq!(p.check_net_bind(9000, AddrClass::Any), RuleAction::Deny);
    }

    #[test]
    fn v2_net_connect_precedence_and_limits() {
        let everywhere =
            AbiRule::net_connect(RuleAction::Allow, [0, 0, 0, 0], 0, &ports(&[(1, 65535)]))
                .unwrap();
        let lan_deny =
            AbiRule::net_connect(RuleAction::Deny, [10, 0, 0, 0], 8, &ports(&[(1, 65535)]))
                .unwrap();
        let host_allow = AbiRule::net_connect(
            RuleAction::Allow,
            [10, 0, 2, 2],
            32,
            &ports(&[(53, 53), (80, 80)]),
        )
        .unwrap();
        let p = profile_with(&[everywhere, lan_deny, host_allow]);
        assert_eq!(p.check_net_connect([1, 1, 1, 1], 443), RuleAction::Allow);
        assert_eq!(p.check_net_connect([10, 0, 2, 3], 443), RuleAction::Deny);
        assert_eq!(p.check_net_connect([10, 0, 2, 2], 53), RuleAction::Allow);
        assert_eq!(p.check_net_connect([10, 0, 2, 2], 443), RuleAction::Deny);
        // No connect rule at all ⇒ deny.
        assert_eq!(
            AbiProfile::empty(SUBJECT).check_net_connect([1, 1, 1, 1], 80),
            RuleAction::Deny
        );
        // Limits: rule ceiling, then profile ceiling, then the default.
        let rule_capped =
            AbiRule::statefs(RuleAction::Allow, b"/state/a/").unwrap().with_max_payload(100);
        let inherit = AbiRule::statefs(RuleAction::Allow, b"/state/b/").unwrap();
        let p = profile_with(&[rule_capped, inherit])
            .with_limits(AbiLimits { max_payload: 1000, deadline_ms: 500 });
        assert_eq!(p.check_statefs_put(b"/state/a/x", 100), RuleAction::Allow);
        assert_eq!(p.check_statefs_put(b"/state/a/x", 101), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/b/x", 1000), RuleAction::Allow);
        assert_eq!(p.check_statefs_put(b"/state/b/x", 1001), RuleAction::Deny);
        assert_eq!(p.check_deadline(500), RuleAction::Allow);
        assert_eq!(p.check_deadline(501), RuleAction::Deny);
        assert_eq!(AbiProfile::empty(SUBJECT).check_deadline(u32::MAX), RuleAction::Allow);
    }
}
