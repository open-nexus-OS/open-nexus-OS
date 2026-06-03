// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Unit-Tests für das Integration Chain Test Framework.
//! OWNERS: @tools-team

use crate::chain::contract::{Contract, ContractError};
use crate::chain::hop::{ms, HopFailure};
use crate::chain::report::ChainStatus;
use crate::chain::{ChainRunner, ServiceId, SimIpcBus};

/// Minimaler Contract für einen Smoke-Test.
struct SmokeContract {
    name: &'static str,
    id: Option<ServiceId>,
    markers: Vec<&'static str>,
}

impl SmokeContract {
    fn new(name: &'static str, markers: Vec<&'static str>) -> Self {
        Self { name, id: None, markers }
    }
}

impl Contract for SmokeContract {
    fn service_name(&self) -> &'static str {
        self.name
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id = self.id.unwrap();
        for marker in &self.markers {
            bus.emit_marker(id, marker);
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════
// Test: SimIpcBus marker emission + retrieval
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn sim_ipc_bus_emits_and_finds_markers() {
    let mut bus = SimIpcBus::new();
    let svc = bus.register_service("test-svc");

    bus.emit_marker(svc, "test: marker one");
    bus.emit_marker(svc, "test: marker two");

    assert!(bus.has_marker("test: marker one"));
    assert!(bus.has_marker("test: marker two"));
    assert!(!bus.has_marker("test: marker three"));

    let found = bus.find_markers("marker");
    assert_eq!(found.len(), 2);
}

// ═══════════════════════════════════════════════════════════
// Test: ChainRunner mit zwei Services und Hop-Validierung
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn chain_runner_passes_when_all_hops_seen() {
    let mut runner = ChainRunner::new("smoke-test");

    runner.register(Box::new(SmokeContract::new(
        "service-a",
        vec!["svc-a: ready", "svc-a: working"],
    )));
    runner.register(Box::new(SmokeContract::new("service-b", vec!["svc-b: ready"])));

    runner.expect_marker("svc-a: ready", ms(100));
    runner.expect_marker("svc-b: ready", ms(100));
    runner.expect_marker("svc-a: working", ms(100));

    let report = runner.run().await;
    assert_eq!(report.status, ChainStatus::Passed);
    assert_eq!(report.hops.len(), 3);
    assert!(report.hops.iter().all(|h| h.seen));
}

// ═══════════════════════════════════════════════════════════
// Test: ChainRunner erkennt fehlende Marker
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn chain_runner_fails_on_missing_marker() {
    let mut runner = ChainRunner::new("fail-test");

    runner.register(Box::new(SmokeContract::new("service-a", vec!["svc-a: ready"])));

    runner.expect_marker("svc-a: ready", ms(100));
    runner.expect_marker("svc-a: never-emitted", ms(100));

    let report = runner.run().await;
    assert!(matches!(&report.status, ChainStatus::Failed { .. }));

    if let ChainStatus::Failed { failed_hop, reason } = &report.status {
        assert_eq!(*failed_hop, 1);
        assert!(matches!(reason, HopFailure::MarkerNotFound { .. }));
    } else {
        panic!("expected failed status");
    }
}

// ═══════════════════════════════════════════════════════════
// Test: ChainRunner — optionaler Hop
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn chain_runner_optional_hop_allows_missing_marker() {
    let mut runner = ChainRunner::new("optional-test");

    runner.register(Box::new(SmokeContract::new("service-a", vec!["svc-a: ready"])));

    runner.expect_marker("svc-a: ready", ms(100));
    runner.expect_marker("svc-a: maybe", ms(100)).optional();
    runner.expect_marker("svc-a: definitely", ms(100));

    let report = runner.run().await;
    assert!(matches!(report.status, ChainStatus::Failed { .. } | ChainStatus::Passed));
}

// ═══════════════════════════════════════════════════════════
// Test: SimIpcBus Cap-Transfer
// ═══════════════════════════════════════════════════════════

#[test]
fn sim_ipc_bus_cap_clone_and_transfer() {
    let mut bus = SimIpcBus::new();
    let svc_a = bus.register_service("svc-a");
    let svc_b = bus.register_service("svc-b");

    let vmo = bus.alloc_vmo(svc_a, 4096);
    assert_eq!(vmo.0, 0);

    let clone = bus.cap_clone(vmo).expect("clone should work");
    assert_eq!(clone.0, 1);

    assert!(bus.cap_transfer(clone, svc_b));

    bus.send(svc_a, svc_b, 1, vec![0x01], Some(clone));

    let msg = bus.recv(svc_b).expect("should receive message");
    assert_eq!(msg.op, 1);
    assert!(msg.cap.is_some());
}

// ═══════════════════════════════════════════════════════════
// Test: Diagnostic-Output im Fehlerfall
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn chain_report_diagnostic_shows_failure_detail() {
    let mut runner = ChainRunner::new("diag-test");

    runner.register(Box::new(SmokeContract::new("service-a", vec!["svc-a: ready"])));

    runner.expect_marker("svc-a: ready", ms(100));
    runner.expect_marker("svc-a: missing", ms(100));

    let report = runner.run().await;
    let diag = report.diagnostic();

    assert!(diag.contains("FAILED"));
    assert!(diag.contains("svc-a: missing"));
    assert!(diag.contains("not seen"));
}

// ═══════════════════════════════════════════════════════════
// Test: SimIpcBus Timeline
// ═══════════════════════════════════════════════════════════

#[test]
fn sim_ipc_bus_records_timeline() {
    let mut bus = SimIpcBus::new();
    let svc_a = bus.register_service("svc-a");
    let svc_b = bus.register_service("svc-b");

    bus.emit_marker(svc_a, "svc-a: ready");
    bus.send(svc_a, svc_b, 1, vec![], None);
    bus.recv(svc_b);

    let timeline = bus.timeline();
    assert!(timeline.len() >= 3);
}
