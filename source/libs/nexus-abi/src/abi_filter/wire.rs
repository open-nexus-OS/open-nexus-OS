// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §3 profile wire codecs. v2 (`'A','F',2`): 20-byte
//! header with subject, epoch and flags, an optional 8-byte limits
//! record, then 16-byte rule records followed by their port ranges and
//! prefix bytes. v1 (`'A','F',1`) keeps decoding (epoch 0, transcoded to
//! the v2 rule shape) until every producer emits v2. Every decode is
//! bounded by `MAX_PROFILE_BYTES`, rejects trailing bytes, unknown
//! classes/flags and inconsistent lengths — never truncates. `ingest_*`
//! binds a received profile to the kernel-attributed sender (must be the
//! authority) and the expected subject.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/abi_filter_reject.rs, tests/abi_filter_v2_reject.rs
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use super::{
    AbiFilterError, AbiLimits, AbiProfile, AbiRule, AddrClass, AuthorityServiceId, PortRange,
    RuleAction, SenderServiceId, SubjectServiceId, SyscallClass, MAX_PATH_PREFIX_BYTES,
    MAX_PORT_RANGES_PER_RULE, MAX_PROFILE_BYTES, MAX_RULES,
};

/// Profile magic byte 0.
pub const PROFILE_MAGIC0: u8 = b'A';
/// Profile magic byte 1.
pub const PROFILE_MAGIC1: u8 = b'F';
/// Legacy profile version (two optional rules, first-match precedence at
/// authoring time; decoded into v2 precedence).
pub const PROFILE_VERSION: u8 = 1;
/// RFC-0091 profile version.
pub const PROFILE_VERSION_V2: u8 = 2;

const V1_HEADER: usize = 12;
const V1_RULE_FIXED: usize = 8;
const V2_HEADER: usize = 20;
const V2_LIMITS: usize = 8;
const V2_RULE_FIXED: usize = 16;
const V2_FLAG_HAS_LIMITS: u32 = 1;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(b)
}

/// Legacy v1 encoder: at most one statefs allow prefix and one
/// `net.bind` allow from `min_port` upwards (decodes as loopback-only,
/// RFC-0091 §1 transcoding).
pub fn encode_profile_v1(
    subject_service_id: u64,
    statefs_put_allow_prefix: Option<&[u8]>,
    net_bind_min_port: Option<u16>,
    out: &mut [u8],
) -> Result<usize, AbiFilterError> {
    if let Some(prefix) = statefs_put_allow_prefix {
        if prefix.is_empty() || prefix.len() > MAX_PATH_PREFIX_BYTES {
            return Err(AbiFilterError::PathPrefixOverflow);
        }
    }
    let rule_count =
        statefs_put_allow_prefix.is_some() as usize + net_bind_min_port.is_some() as usize;
    let total =
        V1_HEADER + rule_count * V1_RULE_FIXED + statefs_put_allow_prefix.map_or(0, <[u8]>::len);
    if total > MAX_PROFILE_BYTES || total > out.len() {
        return Err(AbiFilterError::OversizedProfile);
    }
    out[0] = PROFILE_MAGIC0;
    out[1] = PROFILE_MAGIC1;
    out[2] = PROFILE_VERSION;
    out[3] = rule_count as u8;
    out[4..12].copy_from_slice(&subject_service_id.to_le_bytes());
    let mut at = V1_HEADER;
    if let Some(prefix) = statefs_put_allow_prefix {
        out[at] = SyscallClass::StatefsPut as u8;
        out[at + 1] = RuleAction::Allow as u8;
        out[at + 2] = prefix.len() as u8;
        out[at + 3] = 0;
        out[at + 4..at + 8].fill(0);
        at += V1_RULE_FIXED;
        out[at..at + prefix.len()].copy_from_slice(prefix);
        at += prefix.len();
    }
    if let Some(min) = net_bind_min_port {
        out[at] = SyscallClass::NetBind as u8;
        out[at + 1] = RuleAction::Allow as u8;
        out[at + 2] = 0;
        out[at + 3] = 0;
        out[at + 4..at + 6].copy_from_slice(&min.to_le_bytes());
        out[at + 6..at + 8].copy_from_slice(&u16::MAX.to_le_bytes());
        at += V1_RULE_FIXED;
    }
    Ok(at)
}

/// Decodes a version-1 profile (epoch 0). Trailing bytes are malformed.
pub fn decode_profile_v1(bytes: &[u8]) -> Result<AbiProfile, AbiFilterError> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(AbiFilterError::OversizedProfile);
    }
    if bytes.len() < V1_HEADER
        || bytes[0] != PROFILE_MAGIC0
        || bytes[1] != PROFILE_MAGIC1
        || bytes[2] != PROFILE_VERSION
    {
        return Err(AbiFilterError::MalformedProfile);
    }
    let rule_count = bytes[3] as usize;
    if rule_count > MAX_RULES {
        return Err(AbiFilterError::RuleCountOverflow);
    }
    let mut profile = AbiProfile::empty(u64_at(bytes, 4));
    let mut at = V1_HEADER;
    for _ in 0..rule_count {
        if bytes.len() < at + V1_RULE_FIXED {
            return Err(AbiFilterError::MalformedProfile);
        }
        let class = SyscallClass::from_u8(bytes[at]).ok_or(AbiFilterError::InvalidSyscallClass)?;
        let action = RuleAction::from_u8(bytes[at + 1]).ok_or(AbiFilterError::InvalidRuleAction)?;
        let prefix_len = bytes[at + 2] as usize;
        if bytes[at + 3] != 0 {
            return Err(AbiFilterError::MalformedProfile);
        }
        let range = PortRange { min: u16_at(bytes, at + 4), max: u16_at(bytes, at + 6) };
        at += V1_RULE_FIXED;
        let rule = match class {
            SyscallClass::StatefsPut => {
                if prefix_len > MAX_PATH_PREFIX_BYTES {
                    return Err(AbiFilterError::PathPrefixOverflow);
                }
                if bytes.len() < at + prefix_len {
                    return Err(AbiFilterError::MalformedProfile);
                }
                let rule = AbiRule::statefs(action, &bytes[at..at + prefix_len])?;
                at += prefix_len;
                rule
            }
            SyscallClass::NetBind => {
                if prefix_len != 0 {
                    return Err(AbiFilterError::MalformedProfile);
                }
                AbiRule::net_bind(action, AddrClass::Loopback, &[range])?
            }
            SyscallClass::NetConnect => return Err(AbiFilterError::InvalidSyscallClass),
        };
        profile.push_rule(rule)?;
    }
    if at != bytes.len() {
        return Err(AbiFilterError::MalformedProfile);
    }
    Ok(profile)
}

/// Encoded v2 size of `profile`, or `OversizedProfile`.
fn encoded_len_v2(profile: &AbiProfile) -> Result<usize, AbiFilterError> {
    let mut total = V2_HEADER + if profile.limits().is_some() { V2_LIMITS } else { 0 };
    for i in 0..profile.rule_count() {
        let rule = profile.rule(i).ok_or(AbiFilterError::MalformedProfile)?;
        total += V2_RULE_FIXED
            + rule.port_count as usize * 4
            + if rule.syscall == SyscallClass::StatefsPut {
                rule.path_prefix_len as usize
            } else {
                0
            };
    }
    if total > MAX_PROFILE_BYTES {
        return Err(AbiFilterError::OversizedProfile);
    }
    Ok(total)
}

/// Encodes `profile` as RFC-0091 v2; `out` must hold the whole frame.
pub fn encode_profile_v2(profile: &AbiProfile, out: &mut [u8]) -> Result<usize, AbiFilterError> {
    let total = encoded_len_v2(profile)?;
    if total > out.len() {
        return Err(AbiFilterError::OversizedProfile);
    }
    out[0] = PROFILE_MAGIC0;
    out[1] = PROFILE_MAGIC1;
    out[2] = PROFILE_VERSION_V2;
    out[3] = profile.rule_count() as u8;
    out[4..12].copy_from_slice(&profile.subject_service_id().to_le_bytes());
    out[12..16].copy_from_slice(&profile.epoch().to_le_bytes());
    let flags = if profile.limits().is_some() { V2_FLAG_HAS_LIMITS } else { 0 };
    out[16..20].copy_from_slice(&flags.to_le_bytes());
    let mut at = V2_HEADER;
    if let Some(limits) = profile.limits() {
        out[at..at + 4].copy_from_slice(&limits.max_payload.to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&limits.deadline_ms.to_le_bytes());
        at += V2_LIMITS;
    }
    for i in 0..profile.rule_count() {
        let rule = profile.rule(i).ok_or(AbiFilterError::MalformedProfile)?;
        let is_statefs = rule.syscall == SyscallClass::StatefsPut;
        out[at] = rule.syscall as u8;
        out[at + 1] = rule.action as u8;
        out[at + 2] = if is_statefs { rule.path_prefix_len } else { 0 };
        out[at + 3] = if rule.syscall == SyscallClass::NetBind { rule.addr_class as u8 } else { 0 };
        out[at + 4] = rule.port_count;
        out[at + 5] = if rule.syscall == SyscallClass::NetConnect { rule.cidr_len } else { 0 };
        out[at + 6] = 0;
        out[at + 7] = 0;
        out[at + 8..at + 12].copy_from_slice(&rule.cidr);
        out[at + 12..at + 16].copy_from_slice(&rule.max_payload.to_le_bytes());
        at += V2_RULE_FIXED;
        for r in &rule.ports[..rule.port_count as usize] {
            out[at..at + 2].copy_from_slice(&r.min.to_le_bytes());
            out[at + 2..at + 4].copy_from_slice(&r.max.to_le_bytes());
            at += 4;
        }
        if is_statefs {
            let n = rule.path_prefix_len as usize;
            out[at..at + n].copy_from_slice(&rule.path_prefix[..n]);
            at += n;
        }
    }
    Ok(at)
}

/// Decodes a version-2 profile. Unknown flags/classes, reserved bytes,
/// out-of-class fields and trailing bytes all fail closed.
pub fn decode_profile_v2(bytes: &[u8]) -> Result<AbiProfile, AbiFilterError> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(AbiFilterError::OversizedProfile);
    }
    if bytes.len() < V2_HEADER
        || bytes[0] != PROFILE_MAGIC0
        || bytes[1] != PROFILE_MAGIC1
        || bytes[2] != PROFILE_VERSION_V2
    {
        return Err(AbiFilterError::MalformedProfile);
    }
    let rule_count = bytes[3] as usize;
    if rule_count > MAX_RULES {
        return Err(AbiFilterError::RuleCountOverflow);
    }
    let flags = u32_at(bytes, 16);
    if flags & !V2_FLAG_HAS_LIMITS != 0 {
        return Err(AbiFilterError::MalformedProfile);
    }
    let mut profile = AbiProfile::empty(u64_at(bytes, 4)).with_epoch(u32_at(bytes, 12));
    let mut at = V2_HEADER;
    if flags & V2_FLAG_HAS_LIMITS != 0 {
        if bytes.len() < at + V2_LIMITS {
            return Err(AbiFilterError::MalformedProfile);
        }
        profile = profile.with_limits(AbiLimits {
            max_payload: u32_at(bytes, at),
            deadline_ms: u32_at(bytes, at + 4),
        });
        at += V2_LIMITS;
    }
    for _ in 0..rule_count {
        if bytes.len() < at + V2_RULE_FIXED {
            return Err(AbiFilterError::MalformedProfile);
        }
        let class = SyscallClass::from_u8(bytes[at]).ok_or(AbiFilterError::InvalidSyscallClass)?;
        let action = RuleAction::from_u8(bytes[at + 1]).ok_or(AbiFilterError::InvalidRuleAction)?;
        let prefix_len = bytes[at + 2] as usize;
        let addr_class = bytes[at + 3];
        let port_count = bytes[at + 4] as usize;
        let cidr_len = bytes[at + 5];
        if bytes[at + 6] != 0 || bytes[at + 7] != 0 || port_count > MAX_PORT_RANGES_PER_RULE {
            return Err(AbiFilterError::MalformedProfile);
        }
        let mut cidr = [0u8; 4];
        cidr.copy_from_slice(&bytes[at + 8..at + 12]);
        let max_payload = u32_at(bytes, at + 12);
        at += V2_RULE_FIXED;
        if bytes.len() < at + port_count * 4 {
            return Err(AbiFilterError::MalformedProfile);
        }
        let mut ports = [PortRange::default(); MAX_PORT_RANGES_PER_RULE];
        for r in ports.iter_mut().take(port_count) {
            *r = PortRange { min: u16_at(bytes, at), max: u16_at(bytes, at + 2) };
            at += 4;
        }
        let ports = &ports[..port_count];
        let rule = match class {
            SyscallClass::StatefsPut => {
                if addr_class != 0 || port_count != 0 || cidr_len != 0 || cidr != [0; 4] {
                    return Err(AbiFilterError::MalformedProfile);
                }
                if prefix_len > MAX_PATH_PREFIX_BYTES {
                    return Err(AbiFilterError::PathPrefixOverflow);
                }
                if bytes.len() < at + prefix_len {
                    return Err(AbiFilterError::MalformedProfile);
                }
                let rule = AbiRule::statefs(action, &bytes[at..at + prefix_len])?;
                at += prefix_len;
                rule
            }
            SyscallClass::NetBind => {
                if prefix_len != 0 || cidr_len != 0 || cidr != [0; 4] {
                    return Err(AbiFilterError::MalformedProfile);
                }
                let addr =
                    AddrClass::from_u8(addr_class).ok_or(AbiFilterError::InvalidAddrClass)?;
                AbiRule::net_bind(action, addr, ports)?
            }
            SyscallClass::NetConnect => {
                if prefix_len != 0 || addr_class != 0 {
                    return Err(AbiFilterError::MalformedProfile);
                }
                AbiRule::net_connect(action, cidr, cidr_len, ports)?
            }
        };
        profile.push_rule(rule.with_max_payload(max_payload))?;
    }
    if at != bytes.len() {
        return Err(AbiFilterError::MalformedProfile);
    }
    Ok(profile)
}

/// Version-dispatching decoder (v1 ⇒ epoch 0; v2 as-is).
pub fn decode_profile(bytes: &[u8]) -> Result<AbiProfile, AbiFilterError> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(AbiFilterError::OversizedProfile);
    }
    match bytes.get(2) {
        Some(&PROFILE_VERSION) => decode_profile_v1(bytes),
        Some(&PROFILE_VERSION_V2) => decode_profile_v2(bytes),
        _ => Err(AbiFilterError::MalformedProfile),
    }
}

/// Accepts a distributed profile only from the authority and only for
/// the expected subject (both kernel-attributed / caller-known, never
/// taken from the payload alone).
pub fn ingest_distributed_profile(
    bytes: &[u8],
    sender: SenderServiceId,
    authority: AuthorityServiceId,
    expected_subject: SubjectServiceId,
) -> Result<AbiProfile, AbiFilterError> {
    if sender.get() != authority.get() {
        return Err(AbiFilterError::UnauthenticatedProfileDistribution);
    }
    let profile = decode_profile(bytes)?;
    if profile.subject_service_id() != expected_subject.get() {
        return Err(AbiFilterError::SubjectIdentityMismatch);
    }
    Ok(profile)
}

/// Legacy name of [`ingest_distributed_profile`] (raw ids).
pub fn ingest_distributed_profile_v1(
    bytes: &[u8],
    sender_service_id: u64,
    authority_service_id: u64,
    expected_subject_service_id: u64,
) -> Result<AbiProfile, AbiFilterError> {
    ingest_distributed_profile(
        bytes,
        SenderServiceId::new(sender_service_id),
        AuthorityServiceId::new(authority_service_id),
        SubjectServiceId::new(expected_subject_service_id),
    )
}

/// Legacy name of [`ingest_distributed_profile`] (typed ids).
pub fn ingest_distributed_profile_v1_typed(
    bytes: &[u8],
    sender: SenderServiceId,
    authority: AuthorityServiceId,
    expected_subject: SubjectServiceId,
) -> Result<AbiProfile, AbiFilterError> {
    ingest_distributed_profile(bytes, sender, authority, expected_subject)
}
