// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service identity — the compact id every declaration and every route keys on.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: `nexus-init` topology tests (name round-trip) + the crate tests in lib.rs

/// Compact service identifier — maps 1:1 to the canonical service name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum ServiceId {
    /// Virtual file system daemon.
    Vfsd = 1,
    /// Package file system daemon.
    Packagefsd = 2,
    /// Policy enforcement daemon.
    Policyd = 3,
    /// Bundle manager daemon.
    Bundlemgrd = 4,
    /// Update daemon.
    Updated = 5,
    /// System ability manager daemon.
    Samgrd = 6,
    /// Exec daemon.
    Execd = 7,
    /// Key store daemon.
    Keystored = 8,
    /// State file system daemon.
    Statefsd = 9,
    /// Random number generator daemon.
    Rngd = 10,
    /// Timer daemon.
    Timed = 11,
    /// Window manager / compositor daemon.
    Windowd = 12,
    /// Input routing daemon.
    Inputd = 13,
    /// Ability lifecycle manager daemon (RFC-0065).
    Abilitymgr = 14,
    /// GPU driver daemon.
    Gpud = 15,
    /// Network stack daemon.
    Netstackd = 16,
    /// Metrics daemon.
    Metricsd = 17,
    /// Log daemon.
    Logd = 18,
    /// Distributed soft bus daemon.
    Dsoftbusd = 19,
    /// HID raw input daemon.
    Hidrawd = 20,
    /// Touch input daemon.
    Touchd = 21,
    /// Selftest client.
    SelftestClient = 22,
    /// Session manager daemon (RFC-0069 §4 — owns the `session-start` stage).
    Sessiond = 23,
    /// Typed settings registry daemon (TASK-0072 Phase 8 — persists prefs via statefsd).
    Settingsd = 24,
    /// System-internal compute broker (batch parallel work on nexus-workpool;
    /// invisible to apps — no app-facing route, system clients only).
    Pinched = 25,
    /// Input method editor daemon (RFC-0075 — composes keys for the focused
    /// surface; inputd forwards resolved keys, windowd relays focus).
    Imed = 26,
    /// ROUTE-ONLY pseudo id (no process): imed's dedicated OSK-injection
    /// endpoint (RFC-0075 Phase 2). Possession of this route's SEND cap IS
    /// the injection authorization — execd provisions it only to
    /// `nexus.permission.IME` bundles; the selftest harness probes it.
    ImedOsk = 27,
    /// Single boot-state authority (TASK-0050, ADR-0055): A/B slot record +
    /// boot targets + one-shot next_boot; `updated` is a client.
    Bootctld = 28,
    /// THE virtio-blk owner (ADR-0044 end state, TASK-0315): parses the one
    /// GPT disk and serves partition-scoped block IO; statefsd/nxfsd are
    /// its blockproto clients.
    Blkd = 29,
    /// Inbound gateway (RFC-0092 / TASK-0052): the ONE service that binds
    /// NIC-facing ports; fronts declared `[[expose]]` intents.
    Ingressd = 30,
    /// SoC glue owner (RFC-0106 / TASK-0245B): the ONE writer of the syscon and
    /// pinctrl windows; consumers ask for their node by path.
    Socd = 31,
}

impl ServiceId {
    /// Number of entries needed to index a per-service array by `id as usize`
    /// (discriminants are `1..=31`, so the array spans `0..=31`; index 0 is unused).
    pub const COUNT: usize = 32;

    /// Every service identifier, for iterating a per-service routing array.
    pub const ALL: [ServiceId; 31] = [
        Self::Vfsd,
        Self::Packagefsd,
        Self::Policyd,
        Self::Bundlemgrd,
        Self::Updated,
        Self::Samgrd,
        Self::Execd,
        Self::Keystored,
        Self::Statefsd,
        Self::Rngd,
        Self::Timed,
        Self::Windowd,
        Self::Inputd,
        Self::Abilitymgr,
        Self::Gpud,
        Self::Netstackd,
        Self::Metricsd,
        Self::Logd,
        Self::Dsoftbusd,
        Self::Hidrawd,
        Self::Touchd,
        Self::SelftestClient,
        Self::Sessiond,
        Self::Settingsd,
        Self::Pinched,
        Self::Imed,
        Self::ImedOsk,
        Self::Bootctld,
        Self::Blkd,
        Self::Ingressd,
        Self::Socd,
    ];

    /// Look up a service by its canonical name. Returns None for unknown names.
    pub fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"vfsd" => Self::Vfsd,
            b"packagefsd" => Self::Packagefsd,
            b"policyd" => Self::Policyd,
            b"bundlemgrd" => Self::Bundlemgrd,
            b"updated" => Self::Updated,
            b"samgrd" => Self::Samgrd,
            b"execd" => Self::Execd,
            b"keystored" => Self::Keystored,
            b"statefsd" => Self::Statefsd,
            b"rngd" => Self::Rngd,
            b"timed" => Self::Timed,
            b"windowd" => Self::Windowd,
            b"inputd" => Self::Inputd,
            b"abilitymgr" => Self::Abilitymgr,
            b"gpud" => Self::Gpud,
            b"netstackd" => Self::Netstackd,
            b"metricsd" => Self::Metricsd,
            b"logd" => Self::Logd,
            b"dsoftbusd" => Self::Dsoftbusd,
            b"hidrawd" => Self::Hidrawd,
            b"touchd" => Self::Touchd,
            b"selftest-client" => Self::SelftestClient,
            b"sessiond" => Self::Sessiond,
            b"settingsd" => Self::Settingsd,
            b"pinched" => Self::Pinched,
            b"imed" => Self::Imed,
            b"imed-osk" => Self::ImedOsk,
            b"bootctld" => Self::Bootctld,
            b"blkd" => Self::Blkd,
            b"ingressd" => Self::Ingressd,
            b"socd" => Self::Socd,
            _ => return None,
        })
    }

    /// Returns the canonical service name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vfsd => "vfsd",
            Self::Packagefsd => "packagefsd",
            Self::Policyd => "policyd",
            Self::Bundlemgrd => "bundlemgrd",
            Self::Updated => "updated",
            Self::Samgrd => "samgrd",
            Self::Execd => "execd",
            Self::Keystored => "keystored",
            Self::Statefsd => "statefsd",
            Self::Rngd => "rngd",
            Self::Timed => "timed",
            Self::Windowd => "windowd",
            Self::Inputd => "inputd",
            Self::Abilitymgr => "abilitymgr",
            Self::Gpud => "gpud",
            Self::Netstackd => "netstackd",
            Self::Metricsd => "metricsd",
            Self::Logd => "logd",
            Self::Dsoftbusd => "dsoftbusd",
            Self::Hidrawd => "hidrawd",
            Self::Touchd => "touchd",
            Self::SelftestClient => "selftest-client",
            Self::Sessiond => "sessiond",
            Self::Settingsd => "settingsd",
            Self::Pinched => "pinched",
            Self::Imed => "imed",
            Self::ImedOsk => "imed-osk",
            Self::Bootctld => "bootctld",
            Self::Blkd => "blkd",
            Self::Ingressd => "ingressd",
            Self::Socd => "socd",
        }
    }
}
