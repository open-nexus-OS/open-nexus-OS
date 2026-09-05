// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 policy profile v2 — the argument matchers and the
//! precedence engine every enforcement seam (statefsd `put`, netstackd
//! bind/connect) evaluates. Identity is `sender_service_id` from kernel
//! IPC; the matchers are bounded literals (prefix bytes, numeric port
//! ranges, IPv4 CIDRs) — never patterns — so evaluation is
//! O(rules × bytes) and injection-free. Precedence (§2): most specific
//! accepting rule wins, deny beats allow on ties, no rule ⇒ deny, an
//! allow is then checked against `limits`. The wire codecs (v1 legacy +
//! v2) live in `wire.rs` and are re-exported here for API stability.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/abi_filter_reject.rs, tests/abi_filter_v2_reject.rs
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

pub mod wire;

pub use wire::{
    decode_profile, decode_profile_v1, decode_profile_v2, encode_profile_v1, encode_profile_v2,
    ingest_distributed_profile, ingest_distributed_profile_v1, ingest_distributed_profile_v1_typed,
    PROFILE_MAGIC0, PROFILE_MAGIC1, PROFILE_VERSION, PROFILE_VERSION_V2,
};

pub use nexus_wire::policyd::MAX_PROFILE_BYTES;

/// Upper bound of rules per subject (RFC-0091: 24, raised from v1's 16).
pub const MAX_RULES: usize = 24;
/// Longest statefs path prefix a rule may carry.
pub const MAX_PATH_PREFIX_BYTES: usize = 64;
/// Longest statefs path the matcher accepts (longer ⇒ deny, never truncate).
pub const MAX_STATEFS_PATH_BYTES: usize = 128;
/// Default statefs `put` payload ceiling when no `limits` narrows it.
pub const MAX_STATEFS_PUT_BYTES: usize = 4096;
/// Port ranges per `net.bind` / `net.connect` rule.
pub const MAX_PORT_RANGES_PER_RULE: usize = 4;

/// Reject vocabulary shared by codec, ingest and the reject suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiFilterError {
    /// Encoded size exceeds `MAX_PROFILE_BYTES`.
    OversizedProfile,
    /// Bad magic/version, reserved bits, lengths or trailing bytes.
    MalformedProfile,
    /// More than `MAX_RULES` rules.
    RuleCountOverflow,
    /// Empty or longer-than-`MAX_PATH_PREFIX_BYTES` statefs prefix.
    PathPrefixOverflow,
    /// Zero, more than `MAX_PORT_RANGES_PER_RULE`, or inverted port ranges.
    PortRangeOverflow,
    /// Unknown class byte (fails closed).
    InvalidSyscallClass,
    /// Unknown action byte.
    InvalidRuleAction,
    /// Unknown address class byte.
    InvalidAddrClass,
    /// CIDR length > 32 or host bits set.
    InvalidCidr,
    /// A profile whose epoch is older than the one the consumer holds.
    StaleEpoch,
    /// Profile arrived from a sender other than the authority.
    UnauthenticatedProfileDistribution,
    /// Profile names a subject other than the expected one.
    SubjectIdentityMismatch,
}

/// Kernel-attributed sender of an IPC frame (never a payload string).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SenderServiceId(u64);

impl SenderServiceId {
    /// Constructs a sender identity wrapper.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
    /// Raw identity value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The service allowed to distribute profiles (policyd).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityServiceId(u64);

impl AuthorityServiceId {
    /// Constructs an authority identity wrapper.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
    /// Raw identity value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The subject a profile governs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectServiceId(u64);

impl SubjectServiceId {
    /// Constructs a subject identity wrapper.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
    /// Raw identity value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Governed syscall classes (wire `class` byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SyscallClass {
    /// `statefs.put` (path prefix + payload ceiling).
    StatefsPut = 1,
    /// `net.bind` / listen / udp-bind (port ranges + address class).
    NetBind = 2,
    /// `net.connect` (IPv4 CIDR + port ranges).
    NetConnect = 3,
}

impl SyscallClass {
    /// Decodes the wire class byte; unknown classes fail closed.
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::StatefsPut),
            2 => Some(Self::NetBind),
            3 => Some(Self::NetConnect),
            _ => None,
        }
    }
    /// Human-readable operation name for markers/audit lines.
    pub const fn op_name(self) -> &'static str {
        match self {
            Self::StatefsPut => "statefs.put",
            Self::NetBind => "net.bind",
            Self::NetConnect => "net.connect",
        }
    }
}

/// Rule verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RuleAction {
    /// Refuse the operation.
    Deny = 0,
    /// Permit the operation (subject to `limits`).
    Allow = 1,
}

impl RuleAction {
    /// Decodes the wire action byte.
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Deny),
            1 => Some(Self::Allow),
            _ => None,
        }
    }
}

/// Bind address class (`net.bind`): loopback-only or any interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AddrClass {
    /// Loopback interface only.
    Loopback = 0,
    /// Any interface (TASK-0052: needs an exposure intent).
    Any = 1,
}

impl AddrClass {
    /// Decodes the wire address class byte.
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Loopback),
            1 => Some(Self::Any),
            _ => None,
        }
    }
}

/// Inclusive port range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortRange {
    /// Lowest port (inclusive).
    pub min: u16,
    /// Highest port (inclusive).
    pub max: u16,
}

impl PortRange {
    /// Number of ports covered (the specificity measure; fewer = narrower).
    const fn width(self) -> u32 {
        self.max as u32 - self.min as u32 + 1
    }
    const fn contains(self, port: u16) -> bool {
        self.min <= port && port <= self.max
    }
}

/// Subject-wide ceilings (`[abi_profile.<s>.limits]`); `0` = unset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbiLimits {
    /// Payload ceiling for `statefs.put` (bytes).
    pub max_payload: u32,
    /// Per-request deadline ceiling the seam enforces (ms).
    pub deadline_ms: u32,
}

/// One matcher rule. Fields outside the rule's class are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiRule {
    /// Governed class.
    pub syscall: SyscallClass,
    /// Verdict when the matcher accepts.
    pub action: RuleAction,
    /// Literal statefs path prefix bytes.
    pub path_prefix: [u8; MAX_PATH_PREFIX_BYTES],
    /// Used length of `path_prefix`.
    pub path_prefix_len: u8,
    /// `net.bind` address class.
    pub addr_class: AddrClass,
    /// `net.connect` IPv4 network (big-endian bytes).
    pub cidr: [u8; 4],
    /// `net.connect` prefix length (0..=32).
    pub cidr_len: u8,
    /// Port ranges (net classes).
    pub ports: [PortRange; MAX_PORT_RANGES_PER_RULE],
    /// Used length of `ports`.
    pub port_count: u8,
    /// Per-rule payload ceiling; `0` = inherit the profile limit.
    pub max_payload: u32,
}

impl AbiRule {
    /// A neutral rule (statefs deny with an empty prefix — accepts nothing).
    pub const fn empty() -> Self {
        Self {
            syscall: SyscallClass::StatefsPut,
            action: RuleAction::Deny,
            path_prefix: [0u8; MAX_PATH_PREFIX_BYTES],
            path_prefix_len: 0,
            addr_class: AddrClass::Loopback,
            cidr: [0u8; 4],
            cidr_len: 0,
            ports: [PortRange { min: 0, max: 0 }; MAX_PORT_RANGES_PER_RULE],
            port_count: 0,
            max_payload: 0,
        }
    }

    /// A `statefs` rule over a literal path prefix (1..=64 bytes).
    pub fn statefs(action: RuleAction, prefix: &[u8]) -> Result<Self, AbiFilterError> {
        if prefix.is_empty() || prefix.len() > MAX_PATH_PREFIX_BYTES {
            return Err(AbiFilterError::PathPrefixOverflow);
        }
        let mut rule = Self::empty();
        rule.action = action;
        rule.path_prefix[..prefix.len()].copy_from_slice(prefix);
        rule.path_prefix_len = prefix.len() as u8;
        Ok(rule)
    }

    /// A `net.bind` rule over 1..=4 port ranges and an address class.
    pub fn net_bind(
        action: RuleAction,
        addr_class: AddrClass,
        ports: &[PortRange],
    ) -> Result<Self, AbiFilterError> {
        let mut rule = Self::empty();
        rule.syscall = SyscallClass::NetBind;
        rule.action = action;
        rule.addr_class = addr_class;
        rule.set_ports(ports)?;
        Ok(rule)
    }

    /// A `net.connect` rule over an IPv4 CIDR and 1..=4 port ranges.
    pub fn net_connect(
        action: RuleAction,
        cidr: [u8; 4],
        cidr_len: u8,
        ports: &[PortRange],
    ) -> Result<Self, AbiFilterError> {
        if cidr_len > 32 || !cidr_host_bits_clear(cidr, cidr_len) {
            return Err(AbiFilterError::InvalidCidr);
        }
        let mut rule = Self::empty();
        rule.syscall = SyscallClass::NetConnect;
        rule.action = action;
        rule.cidr = cidr;
        rule.cidr_len = cidr_len;
        rule.set_ports(ports)?;
        Ok(rule)
    }

    /// Sets the per-rule payload ceiling (`0` = inherit).
    pub const fn with_max_payload(mut self, max_payload: u32) -> Self {
        self.max_payload = max_payload;
        self
    }

    fn set_ports(&mut self, ports: &[PortRange]) -> Result<(), AbiFilterError> {
        if ports.is_empty() || ports.len() > MAX_PORT_RANGES_PER_RULE {
            return Err(AbiFilterError::PortRangeOverflow);
        }
        for (i, r) in ports.iter().enumerate() {
            if r.min > r.max {
                return Err(AbiFilterError::PortRangeOverflow);
            }
            self.ports[i] = *r;
        }
        self.port_count = ports.len() as u8;
        Ok(())
    }

    fn prefix(&self) -> &[u8] {
        &self.path_prefix[..(self.path_prefix_len as usize).min(MAX_PATH_PREFIX_BYTES)]
    }

    fn port_ranges(&self) -> &[PortRange] {
        &self.ports[..(self.port_count as usize).min(MAX_PORT_RANGES_PER_RULE)]
    }

    /// Narrowest accepting range width, or `None` when no range contains `port`.
    fn narrowest_port_match(&self, port: u16) -> Option<u32> {
        self.port_ranges().iter().filter(|r| r.contains(port)).map(|r| r.width()).min()
    }
}

/// `true` when the bits beyond `cidr_len` are zero (a canonical network).
pub const fn cidr_host_bits_clear(cidr: [u8; 4], cidr_len: u8) -> bool {
    let addr = u32::from_be_bytes(cidr);
    let mask = if cidr_len == 0 { 0 } else { u32::MAX << (32 - cidr_len as u32) };
    addr & !mask == 0
}

/// A decoded per-subject profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiProfile {
    subject_service_id: u64,
    epoch: u32,
    limits: Option<AbiLimits>,
    rule_count: u8,
    rules: [AbiRule; MAX_RULES],
}

/// Specificity of an accepting rule: higher wins; equal ⇒ deny beats allow.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Specificity(u32, u32);

impl AbiProfile {
    /// The deny-everything profile (no rules, no limits, epoch 0).
    pub const fn empty(subject_service_id: u64) -> Self {
        Self {
            subject_service_id,
            epoch: 0,
            limits: None,
            rule_count: 0,
            rules: [AbiRule::empty(); MAX_RULES],
        }
    }

    /// Sets the authored epoch.
    pub const fn with_epoch(mut self, epoch: u32) -> Self {
        self.epoch = epoch;
        self
    }

    /// Sets the subject-wide limits record.
    pub const fn with_limits(mut self, limits: AbiLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Governed subject.
    pub const fn subject_service_id(&self) -> u64 {
        self.subject_service_id
    }

    /// Authored epoch (0 for v1 profiles).
    pub const fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Subject-wide limits, when authored.
    pub const fn limits(&self) -> Option<AbiLimits> {
        self.limits
    }

    /// Number of rules.
    pub const fn rule_count(&self) -> usize {
        self.rule_count as usize
    }

    /// Rule at `idx`, when present.
    pub fn rule(&self, idx: usize) -> Option<&AbiRule> {
        if idx < self.rule_count() {
            Some(&self.rules[idx])
        } else {
            None
        }
    }

    /// Appends a rule; the count bound is the only reject.
    pub fn push_rule(&mut self, rule: AbiRule) -> Result<(), AbiFilterError> {
        let idx = self.rule_count as usize;
        if idx >= MAX_RULES {
            return Err(AbiFilterError::RuleCountOverflow);
        }
        self.rules[idx] = rule;
        self.rule_count += 1;
        Ok(())
    }

    fn rules(&self) -> &[AbiRule] {
        &self.rules[..self.rule_count()]
    }

    /// Consumer-side monotone check: a profile older than the cached
    /// epoch is stale and must not replace the cached one.
    pub const fn check_replaces_epoch(&self, cached_epoch: u32) -> Result<(), AbiFilterError> {
        if self.epoch < cached_epoch {
            Err(AbiFilterError::StaleEpoch)
        } else {
            Ok(())
        }
    }

    /// Precedence resolution over the accepting rules of one evaluation.
    fn resolve<F>(&self, class: SyscallClass, accept: F) -> Option<(RuleAction, u32)>
    where
        F: Fn(&AbiRule) -> Option<Specificity>,
    {
        let mut best: Option<(Specificity, RuleAction, u32)> = None;
        for rule in self.rules().iter().filter(|r| r.syscall == class) {
            let Some(spec) = accept(rule) else { continue };
            best = Some(match best {
                None => (spec, rule.action, rule.max_payload),
                Some((cur, action, limit)) => {
                    if spec > cur {
                        (spec, rule.action, rule.max_payload)
                    } else if spec == cur && rule.action == RuleAction::Deny {
                        (cur, RuleAction::Deny, rule.max_payload)
                    } else {
                        (cur, action, limit)
                    }
                }
            });
        }
        best.map(|(_, action, limit)| (action, limit))
    }

    /// Effective payload ceiling: rule limit, else profile limit, else default.
    fn payload_ceiling(&self, rule_limit: u32) -> usize {
        if rule_limit != 0 {
            return rule_limit as usize;
        }
        match self.limits {
            Some(l) if l.max_payload != 0 => l.max_payload as usize,
            _ => MAX_STATEFS_PUT_BYTES,
        }
    }

    /// `statefs.put(path, payload_len)` decision.
    pub fn check_statefs_put(&self, path: &[u8], payload_len: usize) -> RuleAction {
        if !statefs_path_is_canonical(path) {
            return RuleAction::Deny;
        }
        let decision = self.resolve(SyscallClass::StatefsPut, |rule| {
            let prefix = rule.prefix();
            (!prefix.is_empty() && path.starts_with(prefix))
                .then_some(Specificity(prefix.len() as u32, 0))
        });
        match decision {
            Some((RuleAction::Allow, limit)) if payload_len <= self.payload_ceiling(limit) => {
                RuleAction::Allow
            }
            _ => RuleAction::Deny,
        }
    }

    /// `net.bind(port, address class)` decision. A loopback-only rule
    /// never accepts an `Any` bind; an `Any` rule accepts both.
    pub fn check_net_bind(&self, port: u16, addr: AddrClass) -> RuleAction {
        let decision = self.resolve(SyscallClass::NetBind, |rule| {
            if rule.addr_class == AddrClass::Loopback && addr == AddrClass::Any {
                return None;
            }
            let width = rule.narrowest_port_match(port)?;
            let addr_rank = if rule.addr_class == AddrClass::Loopback { 1 } else { 0 };
            Some(Specificity(u32::MAX - width, addr_rank))
        });
        match decision {
            Some((RuleAction::Allow, _)) => RuleAction::Allow,
            _ => RuleAction::Deny,
        }
    }

    /// `net.connect(addr, port)` decision (IPv4).
    pub fn check_net_connect(&self, addr: [u8; 4], port: u16) -> RuleAction {
        let decision = self.resolve(SyscallClass::NetConnect, |rule| {
            if !cidr_contains(rule.cidr, rule.cidr_len, addr) {
                return None;
            }
            let width = rule.narrowest_port_match(port)?;
            Some(Specificity(rule.cidr_len as u32, u32::MAX - width))
        });
        match decision {
            Some((RuleAction::Allow, _)) => RuleAction::Allow,
            _ => RuleAction::Deny,
        }
    }

    /// `limits.deadline_ms` check: a request asking for more than the
    /// ceiling is denied (reason `limit`); no ceiling ⇒ allow.
    pub const fn check_deadline(&self, deadline_ms: u32) -> RuleAction {
        match self.limits {
            Some(l) if l.deadline_ms != 0 && deadline_ms > l.deadline_ms => RuleAction::Deny,
            _ => RuleAction::Allow,
        }
    }
}

/// `true` when `addr` lies inside `cidr/cidr_len`.
pub const fn cidr_contains(cidr: [u8; 4], cidr_len: u8, addr: [u8; 4]) -> bool {
    if cidr_len > 32 {
        return false;
    }
    let mask = if cidr_len == 0 { 0 } else { u32::MAX << (32 - cidr_len as u32) };
    (u32::from_be_bytes(cidr) ^ u32::from_be_bytes(addr)) & mask == 0
}

/// Canonical statefs path: bounded, absolute, no NUL, no empty or `.`/`..`
/// segments — the injection surface `test_reject_argument_injection`
/// closes (statefsd canonicalizes too; the matcher never trusts that).
pub fn statefs_path_is_canonical(path: &[u8]) -> bool {
    if path.is_empty() || path.len() > MAX_STATEFS_PATH_BYTES || path[0] != b'/' {
        return false;
    }
    if path.contains(&0) {
        return false;
    }
    path[1..].split(|b| *b == b'/').all(|seg| !seg.is_empty() && seg != b"." && seg != b"..")
}
