// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Config v1 service contract for canonical snapshot reads, subscriber updates, and honest 2PC reload orchestration.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 unit tests covering API alignment, subscriber behavior, and rollback/abort honesty proofs.
//! ADR: docs/adr/0017-service-architecture.md

#![forbid(unsafe_code)]

use nexus_config::{build_effective_snapshot, ConfigError, EffectiveSnapshot, LayerInputs};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveView {
    pub version: String,
    pub capnp_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveJsonView {
    pub version: String,
    pub derived_json: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadReport {
    pub committed: bool,
    pub from_version: String,
    pub candidate_version: String,
    pub active_version: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUpdate {
    pub version: String,
    pub capnp_bytes: Vec<u8>,
    pub derived_json: Value,
}

#[derive(Debug, Error)]
pub enum ReloadError {
    #[error("config validation failed: {0}")]
    Validation(#[from] ConfigError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumerFailure {
    Reject(String),
    Timeout,
    CommitFailed(String),
}

impl ConsumerFailure {
    fn label(&self) -> String {
        match self {
            Self::Reject(reason) => format!("prepare_reject:{reason}"),
            Self::Timeout => "prepare_timeout".to_string(),
            Self::CommitFailed(reason) => format!("commit_failed:{reason}"),
        }
    }
}

pub trait ConfigConsumer {
    fn prepare(&mut self, candidate: &EffectiveSnapshot) -> Result<(), ConsumerFailure>;
    fn commit(&mut self, candidate: &EffectiveSnapshot) -> Result<(), ConsumerFailure>;
    fn abort(&mut self, candidate: &EffectiveSnapshot);
}

pub trait ConfigSubscriber {
    fn on_update(&mut self, update: &ConfigUpdate);
}

pub struct Configd {
    active: EffectiveSnapshot,
    consumers: Vec<Box<dyn ConfigConsumer>>,
    subscribers: Vec<Box<dyn ConfigSubscriber>>,
}

impl Configd {
    pub fn new(initial_layers: LayerInputs) -> Result<Self, ReloadError> {
        let active = build_effective_snapshot(initial_layers)?;
        Ok(Self {
            active,
            consumers: Vec::new(),
            subscribers: Vec::new(),
        })
    }

    pub fn register_consumer(&mut self, consumer: Box<dyn ConfigConsumer>) {
        self.consumers.push(consumer);
    }

    pub fn subscribe(&mut self, subscriber: Box<dyn ConfigSubscriber>) {
        self.subscribers.push(subscriber);
    }

    pub fn get_effective(&self) -> EffectiveView {
        EffectiveView {
            version: self.active.version.clone(),
            capnp_bytes: self.active.capnp_bytes.clone(),
        }
    }

    pub fn get_effective_json(&self) -> EffectiveJsonView {
        EffectiveJsonView {
            version: self.active.version.clone(),
            derived_json: self.active.merged_json.clone(),
        }
    }

    pub fn reload(&mut self, layers: LayerInputs) -> Result<ReloadReport, ReloadError> {
        let candidate = build_effective_snapshot(layers)?;
        let from_version = self.active.version.clone();
        let candidate_version = candidate.version.clone();

        let mut prepared: Vec<usize> = Vec::new();
        for (idx, consumer) in self.consumers.iter_mut().enumerate() {
            if let Err(failure) = consumer.prepare(&candidate) {
                for prepared_idx in &prepared {
                    self.consumers[*prepared_idx].abort(&candidate);
                }
                return Ok(ReloadReport {
                    committed: false,
                    from_version,
                    candidate_version,
                    active_version: self.active.version.clone(),
                    reason: Some(failure.label()),
                });
            }
            prepared.push(idx);
        }

        for prepared_idx in &prepared {
            if let Err(failure) = self.consumers[*prepared_idx].commit(&candidate) {
                for rollback_idx in &prepared {
                    self.consumers[*rollback_idx].abort(&candidate);
                }
                return Ok(ReloadReport {
                    committed: false,
                    from_version,
                    candidate_version,
                    active_version: self.active.version.clone(),
                    reason: Some(failure.label()),
                });
            }
        }

        self.active = candidate;
        let update = ConfigUpdate {
            version: self.active.version.clone(),
            capnp_bytes: self.active.capnp_bytes.clone(),
            derived_json: self.active.merged_json.clone(),
        };
        for subscriber in &mut self.subscribers {
            subscriber.on_update(&update);
        }
        Ok(ReloadReport {
            committed: true,
            from_version,
            candidate_version: self.active.version.clone(),
            active_version: self.active.version.clone(),
            reason: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_config::decode_effective_capnp;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Mode {
        Happy,
        RejectPrepare,
        TimeoutPrepare,
        FailCommit,
    }

    struct MockConsumer {
        mode: Mode,
        prepare_calls: usize,
        commit_calls: usize,
        abort_calls: usize,
    }

    struct MockSubscriber {
        updates: Arc<Mutex<Vec<String>>>,
    }

    impl MockSubscriber {
        fn new(updates: Arc<Mutex<Vec<String>>>) -> Self {
            Self { updates }
        }
    }

    impl MockConsumer {
        fn new(mode: Mode) -> Self {
            Self {
                mode,
                prepare_calls: 0,
                commit_calls: 0,
                abort_calls: 0,
            }
        }
    }

    impl ConfigConsumer for MockConsumer {
        fn prepare(&mut self, _candidate: &EffectiveSnapshot) -> Result<(), ConsumerFailure> {
            self.prepare_calls += 1;
            match self.mode {
                Mode::Happy | Mode::FailCommit => Ok(()),
                Mode::RejectPrepare => Err(ConsumerFailure::Reject("policy_denied".to_string())),
                Mode::TimeoutPrepare => Err(ConsumerFailure::Timeout),
            }
        }

        fn commit(&mut self, _candidate: &EffectiveSnapshot) -> Result<(), ConsumerFailure> {
            self.commit_calls += 1;
            match self.mode {
                Mode::FailCommit => Err(ConsumerFailure::CommitFailed("io_error".to_string())),
                _ => Ok(()),
            }
        }

        fn abort(&mut self, _candidate: &EffectiveSnapshot) {
            self.abort_calls += 1;
        }
    }

    impl ConfigSubscriber for MockSubscriber {
        fn on_update(&mut self, update: &ConfigUpdate) {
            self.updates
                .lock()
                .expect("subscriber lock")
                .push(update.version.clone());
        }
    }

    fn base_layers() -> LayerInputs {
        LayerInputs {
            defaults: json!({
                "dsoftbus": { "transport": "auto", "max_peers": 32 },
                "metrics": { "enabled": true, "flush_interval_ms": 1000 },
                "tracing": { "level": "info", "sample_rate_per_mille": 100 },
                "security_sandbox": { "default_profile": "base", "max_caps": 16 },
                "sched": { "default_qos": "normal", "runqueue_slice_ms": 10 }
            }),
            system: json!({}),
            state: json!({}),
            env: json!({}),
        }
    }

    #[test]
    fn test_get_effective_and_json_are_semantically_aligned() {
        let cfg = Configd::new(base_layers()).expect("configd init");
        let capnp_view = cfg.get_effective();
        let json_view = cfg.get_effective_json();

        assert_eq!(capnp_view.version, json_view.version);
        let decoded = decode_effective_capnp(&capnp_view.capnp_bytes).expect("decode capnp");
        let decoded_json = serde_json::to_value(decoded).expect("to value");
        assert_eq!(decoded_json, json_view.derived_json);
    }

    #[test]
    fn test_abort_2pc_on_prepare_reject_and_keep_previous_version() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        let previous = cfg.get_effective().version;
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::RejectPrepare)));

        let mut changed = base_layers();
        changed.env = json!({ "tracing": { "level": "debug" } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(!report.committed);
        assert_eq!(report.active_version, previous);
        assert_eq!(cfg.get_effective().version, previous);
        assert_eq!(
            report.reason.as_deref(),
            Some("prepare_reject:policy_denied")
        );
    }

    #[test]
    fn test_abort_2pc_on_prepare_timeout_and_keep_previous_version() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        let previous = cfg.get_effective().version;
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::TimeoutPrepare)));

        let mut changed = base_layers();
        changed.env = json!({ "metrics": { "enabled": false } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(!report.committed);
        assert_eq!(report.active_version, previous);
        assert_eq!(cfg.get_effective().version, previous);
        assert_eq!(report.reason.as_deref(), Some("prepare_timeout"));
    }

    #[test]
    fn test_commit_failure_rolls_back_previous_version() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        let previous = cfg.get_effective().version;
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::FailCommit)));

        let mut changed = base_layers();
        changed.state = json!({ "security_sandbox": { "max_caps": 17 } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(!report.committed);
        assert_eq!(report.active_version, previous);
        assert_eq!(cfg.get_effective().version, previous);
        assert_eq!(report.reason.as_deref(), Some("commit_failed:io_error"));
    }

    #[test]
    fn test_no_fake_success_marker_without_state_transition() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        let previous = cfg.get_effective().version;
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::RejectPrepare)));

        let mut changed = base_layers();
        changed.system = json!({ "dsoftbus": { "transport": "quic" } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(!report.committed);
        assert_eq!(report.active_version, previous);
        assert_eq!(cfg.get_effective().version, previous);
    }

    #[test]
    fn test_commit_path_advances_effective_version() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        let previous = cfg.get_effective().version;
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::Happy)));

        let mut changed = base_layers();
        changed.state = json!({ "sched": { "runqueue_slice_ms": 22 } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(report.committed);
        assert_ne!(report.active_version, previous);
        assert_eq!(cfg.get_effective().version, report.active_version);
    }

    #[test]
    fn test_subscribe_notified_only_on_committed_update() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::Happy)));
        let updates = Arc::new(Mutex::new(Vec::new()));
        cfg.subscribe(Box::new(MockSubscriber::new(updates.clone())));

        let mut changed = base_layers();
        changed.state = json!({ "sched": { "runqueue_slice_ms": 21 } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(report.committed);
        let captured = updates.lock().expect("updates lock");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0], report.active_version);
    }

    #[test]
    fn test_subscribe_not_notified_on_aborted_update() {
        let mut cfg = Configd::new(base_layers()).expect("configd init");
        cfg.register_consumer(Box::new(MockConsumer::new(Mode::RejectPrepare)));
        let updates = Arc::new(Mutex::new(Vec::new()));
        cfg.subscribe(Box::new(MockSubscriber::new(updates.clone())));

        let mut changed = base_layers();
        changed.system = json!({ "dsoftbus": { "transport": "quic" } });
        let report = cfg.reload(changed).expect("reload executes");

        assert!(!report.committed);
        let captured = updates.lock().expect("updates lock");
        assert!(captured.is_empty());
    }
}
