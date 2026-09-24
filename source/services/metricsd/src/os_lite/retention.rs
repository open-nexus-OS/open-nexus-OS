// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: metricsd's retention sink — every accepted metric and span goes to the
//! statefs WAL (plus 10 s / 60 s rollups, with GC), and each writer's FIRST retained
//! record is verified with a second put and announced with the writer's id
//! (`retention wal verified sender=0x…`, TASK-0286 P5): a proof anchored to the
//! writer's own record, independent of who wrote first and of logd's ring depth.
//! Split out of `os_lite.rs` (structure gate).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker `SELFTEST: metrics retention ok` (the harness finds its own
//!   announcement from before its first metric)

use alloc::format;

use nexus_ipc::KernelClient;
use statefs::client::StatefsClient;

use super::{declared, emit_line};
use crate::statefs_io::{put_with_retries, statefs_delete_nonblocking};
use crate::{RetentionEngine, RetentionEventKind, RuntimeLimits};

/// Writers whose retention is verified and announced one by one (`retention wal verified
/// sender=…`); later writers are retained all the same, only not announced.
const MAX_VERIFIED_WRITERS: usize = 16;

pub(super) struct RetentionSink {
    limits: RuntimeLimits,
    engine: RetentionEngine,
    client: Option<KernelClient>,
    proof_client: Option<StatefsClient>,
    wal_proof_emitted: bool,
    /// Writers whose first retained record was verified and announced (bounded).
    verified: [u64; MAX_VERIFIED_WRITERS],
    verified_len: usize,
    rollup_10s_proof_emitted: bool,
    rollup_60s_proof_emitted: bool,
}

impl RetentionSink {
    pub(super) fn new(limits: RuntimeLimits) -> Self {
        let client = if limits.retention_enabled {
            KernelClient::new_with_slots(declared::STATEFSD.send, declared::REPLY.recv).ok()
        } else {
            None
        };
        let proof_client = if limits.retention_enabled {
            let client =
                KernelClient::new_with_slots(declared::STATEFSD.send, declared::REPLY.recv).ok();
            let reply =
                KernelClient::new_with_slots(declared::REPLY.send, declared::REPLY.recv).ok();
            match (client, reply) {
                (Some(c), Some(r)) => Some(StatefsClient::from_clients(c, r)),
                _ => None,
            }
        } else {
            None
        };
        if limits.retention_enabled && client.is_none() {
            emit_line("metricsd: retention statefs unavailable");
        }
        Self {
            limits,
            engine: RetentionEngine::new(limits),
            client,
            proof_client,
            wal_proof_emitted: false,
            verified: [0; MAX_VERIFIED_WRITERS],
            verified_len: 0,
            rollup_10s_proof_emitted: false,
            rollup_60s_proof_emitted: false,
        }
    }

    pub(super) fn record_metric(&mut self, sender: u64, record: &str) {
        self.record(sender, RetentionEventKind::Metric, record.as_bytes());
    }

    pub(super) fn record_span(&mut self, sender: u64, record: &str) {
        self.record(sender, RetentionEventKind::Span, record.as_bytes());
    }

    fn record(&mut self, sender: u64, kind: RetentionEventKind, record: &[u8]) {
        let Some(update) = self.engine.append(kind, record) else {
            return;
        };
        let Some(client) = self.client.as_ref() else {
            return;
        };
        let retries = match kind {
            RetentionEventKind::Metric => self.limits.retention_best_effort_retries,
            RetentionEventKind::Span => self.limits.retention_critical_retries,
        };
        let key = format!("/state/observability/metricsd/wal/seg_{}", update.wal_slot);
        let wal_ok = put_with_retries(client, key.as_str(), &update.wal_bytes, retries);
        if !wal_ok {
            return;
        }
        if !self.wal_proof_emitted {
            self.wal_proof_emitted = true;
            if !nexus_abi::service_trace() {
                nexus_log::info("metricsd", |line| {
                    line.text("retention wal active");
                });
            }
        }
        // The evidence is per writer (TASK-0286 P5): each sender's FIRST retained record is
        // verified with a second put and announced with the sender — a proof anchored to the
        // writer's own record, whoever wrote first and however deep logd's ring is. (One
        // announcement for the whole service used to prove the harness's retention only when
        // the harness happened to be the first writer.)
        let known = self.verified[..self.verified_len].contains(&sender);
        if !known && self.verified_len < MAX_VERIFIED_WRITERS {
            if let Some(proof) = self.proof_client.as_ref() {
                if proof.put(key.as_str(), &update.wal_bytes).is_ok() {
                    self.verified[self.verified_len] = sender;
                    self.verified_len += 1;
                    if !nexus_abi::service_trace() {
                        nexus_log::info("metricsd", |line| {
                            line.text("retention wal verified sender=");
                            line.hex(sender);
                        });
                    }
                }
            }
        }
        if let Some(rollup_10s) = update.rollup_10s.as_ref() {
            let key =
                format!("/state/observability/metricsd/rollup/10s/w_{}", rollup_10s.window_id);
            let _ = put_with_retries(
                client,
                key.as_str(),
                &rollup_10s.bytes,
                self.limits.retention_best_effort_retries,
            );
            if !self.rollup_10s_proof_emitted {
                self.rollup_10s_proof_emitted = true;
                nexus_log::info("metricsd", |line| {
                    line.text("retention rollup 10s active");
                });
            }
        }
        for window_id in update.gc_rollup_10s.iter().copied() {
            let key = format!("/state/observability/metricsd/rollup/10s/w_{}", window_id);
            let _ = statefs_delete_nonblocking(client, key.as_str());
        }
        if let Some(rollup_60s) = update.rollup_60s.as_ref() {
            let key =
                format!("/state/observability/metricsd/rollup/60s/w_{}", rollup_60s.window_id);
            let _ = put_with_retries(
                client,
                key.as_str(),
                &rollup_60s.bytes,
                self.limits.retention_critical_retries,
            );
            if !self.rollup_60s_proof_emitted {
                self.rollup_60s_proof_emitted = true;
                nexus_log::info("metricsd", |line| {
                    line.text("retention rollup 60s active");
                });
            }
        }
        for window_id in update.gc_rollup_60s.iter().copied() {
            let key = format!("/state/observability/metricsd/rollup/60s/w_{}", window_id);
            let _ = statefs_delete_nonblocking(client, key.as_str());
        }
    }
}
