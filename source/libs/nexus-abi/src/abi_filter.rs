// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deterministic userspace ABI syscall guardrail profile format and matcher.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host unit tests (`cargo test -p nexus-abi -- reject --nocapture`)
//! INVARIANTS:
//! - bounded profile decode and bounded matcher cost
//! - deny-by-default if no rule matches
//! - profile distribution must be authority-authenticated and subject-bound

/// First profile magic byte.
pub const PROFILE_MAGIC0: u8 = b'A';
/// Second profile magic byte.
pub const PROFILE_MAGIC1: u8 = b'F';
/// Profile wire version.
pub const PROFILE_VERSION: u8 = 1;

/// Maximum encoded profile bytes accepted by the decoder.
pub const MAX_PROFILE_BYTES: usize = 512;
/// Maximum number of rules accepted by the decoder.
pub const MAX_RULES: usize = 16;
/// Maximum bytes for a statefs path-prefix rule matcher.
pub const MAX_PATH_PREFIX_BYTES: usize = 64;
/// Maximum accepted statefs key bytes for guardrail matching.
pub const MAX_STATEFS_PATH_BYTES: usize = 128;
/// Maximum accepted statefs write payload bytes for guardrail matching.
pub const MAX_STATEFS_PUT_BYTES: usize = 4096;

/// Stable syscall operation label used by marker/log surfaces.
pub const SYSCALL_OP_STATEFS_PUT: &str = "statefs.put";
/// Stable syscall operation label used by marker/log surfaces.
pub const SYSCALL_OP_NET_BIND: &str = "net.bind";

/// Errors returned by profile distribution, decode, and matching helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiFilterError {
    /// Profile payload exceeds [`MAX_PROFILE_BYTES`].
    OversizedProfile,
    /// Profile payload is malformed.
    MalformedProfile,
    /// Encoded rule count exceeds [`MAX_RULES`].
    RuleCountOverflow,
    /// Encoded path prefix exceeds [`MAX_PATH_PREFIX_BYTES`].
    PathPrefixOverflow,
    /// Encoded syscall class is unknown.
    InvalidSyscallClass,
    /// Encoded rule action is unknown.
    InvalidRuleAction,
    /// Profile sender does not match the expected authority identity.
    UnauthenticatedProfileDistribution,
    /// Profile subject identity does not match the expected subject.
    SubjectIdentityMismatch,
}

/// Sender identity derived from kernel IPC metadata (`sender_service_id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SenderServiceId(u64);

impl SenderServiceId {
    /// Constructs a sender identity wrapper.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw kernel-derived service identity.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Profile authority identity (typically `policyd`) used for authentication checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityServiceId(u64);

impl AuthorityServiceId {
    /// Constructs an authority identity wrapper.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw authority service identity.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Expected subject identity for which the profile is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectServiceId(u64);

impl SubjectServiceId {
    /// Constructs a subject identity wrapper.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw subject service identity.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Supported syscall classes for v1 userspace ABI filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SyscallClass {
    /// Statefs `put`-style write operation.
    StatefsPut = 1,
    /// Network bind operation.
    NetBind = 2,
}

impl SyscallClass {
    fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::StatefsPut),
            2 => Some(Self::NetBind),
            _ => None,
        }
    }

    /// Returns the stable operation label used for markers/logs.
    pub const fn op_name(self) -> &'static str {
        match self {
            Self::StatefsPut => SYSCALL_OP_STATEFS_PUT,
            Self::NetBind => SYSCALL_OP_NET_BIND,
        }
    }
}

/// Rule decision action.
#[must_use = "use the filter decision to enforce allow/deny behavior"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RuleAction {
    /// Deny the operation.
    Deny = 0,
    /// Allow the operation.
    Allow = 1,
}

impl RuleAction {
    fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Deny),
            1 => Some(Self::Allow),
            _ => None,
        }
    }
}

/// One bounded v1 filter rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiRule {
    /// Syscall class selector.
    pub syscall: SyscallClass,
    /// Rule decision.
    pub action: RuleAction,
    /// Prefix bytes used for statefs path matching.
    pub path_prefix: [u8; MAX_PATH_PREFIX_BYTES],
    /// Number of valid bytes in [`Self::path_prefix`].
    pub path_prefix_len: u8,
    /// Inclusive lower port bound for net-bind matching.
    pub port_min: u16,
    /// Inclusive upper port bound for net-bind matching.
    pub port_max: u16,
}

impl AbiRule {
    /// Returns an empty deny rule placeholder.
    pub const fn empty() -> Self {
        Self {
            syscall: SyscallClass::StatefsPut,
            action: RuleAction::Deny,
            path_prefix: [0u8; MAX_PATH_PREFIX_BYTES],
            path_prefix_len: 0,
            port_min: 0,
            port_max: 0,
        }
    }

    fn matches_statefs_put(&self, path: &[u8], payload_len: usize) -> bool {
        if self.syscall != SyscallClass::StatefsPut {
            return false;
        }
        if path.len() > MAX_STATEFS_PATH_BYTES || payload_len > MAX_STATEFS_PUT_BYTES {
            return false;
        }
        let prefix_len = self.path_prefix_len as usize;
        if prefix_len == 0 || prefix_len > path.len() {
            return false;
        }
        self.path_prefix[..prefix_len] == path[..prefix_len]
    }

    fn matches_net_bind(&self, port: u16) -> bool {
        if self.syscall != SyscallClass::NetBind {
            return false;
        }
        port >= self.port_min && port <= self.port_max
    }
}

/// Parsed ABI filter profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiProfile {
    subject_service_id: u64,
    rule_count: u8,
    rules: [AbiRule; MAX_RULES],
}

impl AbiProfile {
    /// Creates an empty deny-by-default profile for `subject_service_id`.
    pub const fn empty(subject_service_id: u64) -> Self {
        Self { subject_service_id, rule_count: 0, rules: [AbiRule::empty(); MAX_RULES] }
    }

    /// Returns the kernel-derived subject identity bound to this profile.
    pub const fn subject_service_id(&self) -> u64 {
        self.subject_service_id
    }

    /// Returns the number of encoded rules.
    pub const fn rule_count(&self) -> usize {
        self.rule_count as usize
    }

    /// Returns the rule at `index`, if present.
    pub fn rule(&self, index: usize) -> Option<&AbiRule> {
        if index < self.rule_count as usize {
            Some(&self.rules[index])
        } else {
            None
        }
    }

    /// Evaluates a statefs put operation against this profile.
    ///
    /// The matcher is first-match-wins and deny-by-default.
    #[must_use = "filter decisions must be checked before issuing syscall wrappers"]
    pub fn check_statefs_put(&self, path: &[u8], payload_len: usize) -> RuleAction {
        let mut i = 0usize;
        while i < self.rule_count as usize {
            let rule = &self.rules[i];
            if rule.matches_statefs_put(path, payload_len) {
                return rule.action;
            }
            i += 1;
        }
        RuleAction::Deny
    }

    /// Evaluates a net bind operation against this profile.
    ///
    /// The matcher is first-match-wins and deny-by-default.
    #[must_use = "filter decisions must be checked before issuing syscall wrappers"]
    pub fn check_net_bind(&self, port: u16) -> RuleAction {
        let mut i = 0usize;
        while i < self.rule_count as usize {
            let rule = &self.rules[i];
            if rule.matches_net_bind(port) {
                return rule.action;
            }
            i += 1;
        }
        RuleAction::Deny
    }

    fn push_rule(&mut self, rule: AbiRule) -> core::result::Result<(), AbiFilterError> {
        let idx = self.rule_count as usize;
        if idx >= MAX_RULES {
            return Err(AbiFilterError::RuleCountOverflow);
        }
        self.rules[idx] = rule;
        self.rule_count = self.rule_count.saturating_add(1);
        Ok(())
    }
}

/// Encodes a bounded v1 profile into `out`.
///
/// The encoded profile is deny-by-default; only explicitly encoded allow rules are accepted.
pub fn encode_profile_v1(
    subject_service_id: u64,
    statefs_put_allow_prefix: Option<&[u8]>,
    net_bind_min_port: Option<u16>,
    out: &mut [u8],
) -> core::result::Result<usize, AbiFilterError> {
    let mut rule_count = 0usize;
    let mut path_len = 0usize;
    if let Some(prefix) = statefs_put_allow_prefix {
        if prefix.is_empty() {
            return Err(AbiFilterError::MalformedProfile);
        }
        if prefix.len() > MAX_PATH_PREFIX_BYTES {
            return Err(AbiFilterError::PathPrefixOverflow);
        }
        path_len = prefix.len();
        rule_count += 1;
    }
    if net_bind_min_port.is_some() {
        rule_count += 1;
    }
    if rule_count > MAX_RULES {
        return Err(AbiFilterError::RuleCountOverflow);
    }
    let required = 12 + (8 * rule_count) + path_len;
    if required > MAX_PROFILE_BYTES || required > out.len() {
        return Err(AbiFilterError::OversizedProfile);
    }

    out[0] = PROFILE_MAGIC0;
    out[1] = PROFILE_MAGIC1;
    out[2] = PROFILE_VERSION;
    out[3] = rule_count as u8;
    out[4..12].copy_from_slice(&subject_service_id.to_le_bytes());
    let mut off = 12usize;

    if let Some(prefix) = statefs_put_allow_prefix {
        out[off] = SyscallClass::StatefsPut as u8;
        out[off + 1] = RuleAction::Allow as u8;
        out[off + 2] = prefix.len() as u8;
        out[off + 3] = 0;
        out[off + 4..off + 6].copy_from_slice(&0u16.to_le_bytes());
        out[off + 6..off + 8].copy_from_slice(&0u16.to_le_bytes());
        off += 8;
        out[off..off + prefix.len()].copy_from_slice(prefix);
        off += prefix.len();
    }

    if let Some(min_port) = net_bind_min_port {
        out[off] = SyscallClass::NetBind as u8;
        out[off + 1] = RuleAction::Allow as u8;
        out[off + 2] = 0;
        out[off + 3] = 0;
        out[off + 4..off + 6].copy_from_slice(&min_port.to_le_bytes());
        out[off + 6..off + 8].copy_from_slice(&u16::MAX.to_le_bytes());
        off += 8;
    }

    Ok(off)
}

/// Decodes a bounded v1 profile payload.
pub fn decode_profile_v1(profile_bytes: &[u8]) -> core::result::Result<AbiProfile, AbiFilterError> {
    if profile_bytes.len() > MAX_PROFILE_BYTES {
        return Err(AbiFilterError::OversizedProfile);
    }
    if profile_bytes.len() < 12 {
        return Err(AbiFilterError::MalformedProfile);
    }
    if profile_bytes[0] != PROFILE_MAGIC0
        || profile_bytes[1] != PROFILE_MAGIC1
        || profile_bytes[2] != PROFILE_VERSION
    {
        return Err(AbiFilterError::MalformedProfile);
    }

    let rule_count = profile_bytes[3] as usize;
    if rule_count > MAX_RULES {
        return Err(AbiFilterError::RuleCountOverflow);
    }
    let subject_service_id = u64::from_le_bytes([
        profile_bytes[4],
        profile_bytes[5],
        profile_bytes[6],
        profile_bytes[7],
        profile_bytes[8],
        profile_bytes[9],
        profile_bytes[10],
        profile_bytes[11],
    ]);

    let mut profile = AbiProfile::empty(subject_service_id);
    let mut off = 12usize;
    let mut i = 0usize;
    while i < rule_count {
        if off + 8 > profile_bytes.len() {
            return Err(AbiFilterError::MalformedProfile);
        }
        let syscall =
            SyscallClass::from_u8(profile_bytes[off]).ok_or(AbiFilterError::InvalidSyscallClass)?;
        let action =
            RuleAction::from_u8(profile_bytes[off + 1]).ok_or(AbiFilterError::InvalidRuleAction)?;
        let prefix_len = profile_bytes[off + 2] as usize;
        if prefix_len > MAX_PATH_PREFIX_BYTES {
            return Err(AbiFilterError::PathPrefixOverflow);
        }
        let port_min = u16::from_le_bytes([profile_bytes[off + 4], profile_bytes[off + 5]]);
        let port_max = u16::from_le_bytes([profile_bytes[off + 6], profile_bytes[off + 7]]);
        off += 8;
        if off + prefix_len > profile_bytes.len() {
            return Err(AbiFilterError::MalformedProfile);
        }
        let mut rule = AbiRule::empty();
        rule.syscall = syscall;
        rule.action = action;
        rule.path_prefix_len = prefix_len as u8;
        rule.port_min = port_min;
        rule.port_max = port_max;
        if prefix_len != 0 {
            rule.path_prefix[..prefix_len].copy_from_slice(&profile_bytes[off..off + prefix_len]);
        }
        off += prefix_len;
        profile.push_rule(rule)?;
        i += 1;
    }

    if off != profile_bytes.len() {
        return Err(AbiFilterError::MalformedProfile);
    }
    Ok(profile)
}

/// Validates and decodes a distributed profile payload.
///
/// Security checks:
/// - sender identity must match the authenticated authority (`sender_service_id`)
/// - decoded profile subject must match the expected local subject identity
pub fn ingest_distributed_profile_v1(
    profile_bytes: &[u8],
    sender_service_id: u64,
    authority_service_id: u64,
    expected_subject_service_id: u64,
) -> core::result::Result<AbiProfile, AbiFilterError> {
    ingest_distributed_profile_v1_typed(
        profile_bytes,
        SenderServiceId::new(sender_service_id),
        AuthorityServiceId::new(authority_service_id),
        SubjectServiceId::new(expected_subject_service_id),
    )
}

/// Typed variant of [`ingest_distributed_profile_v1`] to avoid identity mix-ups at call-sites.
pub fn ingest_distributed_profile_v1_typed(
    profile_bytes: &[u8],
    sender_service_id: SenderServiceId,
    authority_service_id: AuthorityServiceId,
    expected_subject_service_id: SubjectServiceId,
) -> core::result::Result<AbiProfile, AbiFilterError> {
    if sender_service_id.raw() != authority_service_id.raw() {
        return Err(AbiFilterError::UnauthenticatedProfileDistribution);
    }
    let profile = decode_profile_v1(profile_bytes)?;
    if profile.subject_service_id() != expected_subject_service_id.raw() {
        return Err(AbiFilterError::SubjectIdentityMismatch);
    }
    Ok(profile)
}
