---
title: TASK-0049B Reliability v1b: service supervision — declarative tiers + restart/backoff/crash-loop + capability re-resolve
status: Draft
owner: @reliability @runtime
created: 2026-08-18
depends-on:
  - TASK-0049 # exit reasons (decision basis — no heuristics)
follow-up-tasks: []
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Restart/re-resolve decision: docs/adr/0057-service-restart-capability-re-resolve.md
  - Init manifest: docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md
  - PeerClosed signal: docs/rfcs/RFC-0079-ipc-last-sender-eof.md
  - Declarative routes / typed client: docs/rfcs/RFC-0066-production-grade-service-chain-declarative-routing-typed-ipc-inprocess-tests.md
  - Resource sentinel: docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md
  - Standing detectors doctrine: docs/adr/0048-userspace-runtime-integrity-detectors.md
  - Lifecycle state machine (app-side sibling): tasks/TASK-0234-ability-v1_1a-host-lifecycle-state-backoff-killreasons-deterministic.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

RFC-0087 Phase 1. Today nothing supervises a dying service on the OS:
`execd::os_lite::exec_elf(…, _restart)` returns `Unsupported`
(`source/services/execd/src/os_lite.rs:1433`), `init: supervise X restart=always`
is a host-side `println!`, and init-spawned services are observed by nobody.
A working restart loop already exists — but only in execd's host backend
(`std_server.rs:77-201`: registry, reaper thread, `restart_service()`).

Authority split per user decision 2026-08-18: **nexus-init owns supervision
policy** (declaratively, in the RFC-0069 ServiceSpec manifest); **execd stays
mechanics** (spawn/reap/report, RFC-0081). No new daemon.

Restart in a capability microkernel is a protocol, not a respawn: clients hold
caps to the dead instance. ADR-0057 fixes the protocol (samgrd staleness +
client re-resolve on PeerClosed) and the no-rights-drift invariant (restart
re-provisions from declared topology only).

## Goal

1. **Declarative policy**: `ServiceSpec` (`source/init/nexus-init/src/service_topology.rs`)
   gains `criticality` (`critical-boot | critical-session | standard`),
   `restart` policy, and backoff parameters (`initial_ms`, `factor`, `max_ms`,
   crash-loop `window`/`threshold`). Host-tested like the existing topology.
2. **Restart mechanics on OS**: port the proven host loop
   (`execd/src/std_server.rs:77-201`) into the os-lite path; `os_lite.rs:1433`
   loses its `Unsupported` stub. Init detects death of its supervised services
   (via execd exit reports / wait_nohang sweep) and re-provisions through the
   SAME orchestrator path as first boot (RFC-0066 Phase-3 machinery) — no
   second provisioning code.
3. **Backoff + crash-loop cap**: injectable time source (no wall clock in
   tests); decisions consume the **real exit reason** from TASK-0049 — `clean`
   never counts. Cap ⇒ `blocked` + marker
   `init: crash-loop blocked svc=<name> reason=<reason>`. Counters persist as a
   bounded statefs record (envelope from `userspace/statefs/src/envelope.rs`)
   so loops survive reboot.
4. **Capability re-resolve** (ADR-0057): samgrd stale marking + deterministic
   `Stale`-class resolve error + fresh re-registration; client side lands in the
   shared `nexus_ipc::Connection` layer (re-resolve on PeerClosed under RFC-0025
   bounded retry), not per-client copies. Persistent ctrl-plane slots are never
   closed.
5. **No-rights-drift proof**: restart storm (N induced crashes + restarts) with
   the RFC-0013 resource sentinel green afterwards — caps/endpoints/spawn
   pressure flat.
6. **Standing fault injector** (ADR-0048 doctrine): a selftest phase kills a
   live `standard`-tier probe service → asserts restart marker, client
   re-resolve success, and after threshold crashes the crash-loop cap. The
   detector IS the regression test.

## Non-Goals

- App/ability lifecycle FG/BG semantics (TASK-0234/0235 — the app-side state
  machine; share the backoff schedule *shape*, not code ownership).
- Hang/liveness detection beyond exits + PeerClosed (follow-up RFC per RFC-0087).
- Kernel changes (exit reason already landed in TASK-0049).
- windowd/gpud full session re-attach choreography — this task delivers the
  protocol + a probe-service proof; per-service re-attach depth for
  `critical-session` services is specified here as contract text and executed
  where cheap, deferred where it needs scene rebuild work (explicitly listed in
  the ledger at execution time, no silent claims).

## Constraints / invariants (hard requirements)

- Restart re-provisioning ONLY from `ServiceSpec`/`REQUIRED_ROUTES` — no
  authority minted in the restart path (ADR-0057).
- Supervision decisions bind to kernel-attributed identity, never names in
  payloads.
- Backoff schedule deterministic given the same input sequence.
- Supervision path runs under existing QoS/lockclass right-of-way (ADR-0049);
  upgrade to a conserved right belongs to TRACK-TIME-AS-RESOURCE, not here.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (who observes init's children)**: init spawns services via its own
  exec_v2 wrapper; the reaper-of-record is execd (RFC-0081). The exit-report
  edge init needs (execd → init, or init's own wait_nohang sweep over its
  children) must be decided at execution start and recorded here — do NOT build
  both.
- **YELLOW (critical-boot escalation)**: repeated critical-boot failure
  escalates to reboot-into-`safe` per RFC-0087 — but reset lands in TASK-0050.
  Until 0050 ships, escalation emits the decision marker and parks
  (`init: escalation pending (no reset path)`); no fake reboot claims.
- **YELLOW (samgrd surface)**: staleness is additive to the existing registry
  protocol; reject-path tests required (`test_reject_stale_resolve`,
  `test_reject_foreign_reregister` — only the supervised identity may
  re-register its name).

## Contract sources (single source of truth)

- Tiers/policy semantics: RFC-0087 §2; protocol: ADR-0057.
- Topology: `source/init/nexus-init/src/service_topology.rs` +
  `route_table::REQUIRED_ROUTES`.
- Marker contract: `scripts/qemu-test.sh` + proof-manifest.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nexus-init`: spec parsing/validation; backoff schedule exact
  (`[500,1000,2000,4000,8000,15000,15000…]`-style, clamped); crash-loop window
  → `blocked`; `clean` exits never count; persistence record roundtrip.
- `cargo test -p execd`: os-lite restart path (mock transport), exit-report edge.
- `cargo test -p samgrd`: stale → `Stale` error → re-register → fresh resolve;
  reject tests as above.
- `cargo test -p nexus-ipc`: `Connection` re-resolve on PeerClosed (mock), bounded.

### Proof (OS/QEMU) — required

- `init: restart svc=<name> attempt=<n>`
- `SELFTEST: supervision restart ok` (probe killed → restarted → client
  round-trip through re-resolved endpoint succeeds)
- `init: crash-loop blocked svc=<name> reason=<reason>` +
  `SELFTEST: crash-loop cap ok`
- `KSELFTEST: resource sentinel ok` green in the same run (restart storm; drift
  = failure)
- Double boot (`NEXUS_KEEP_BLK=1`): crash-loop counter survives reboot
  (`SELFTEST: crash-loop persist ok`)

## Touched paths (allowlist)

- `source/init/nexus-init/src/` (spec fields, supervision loop, markers)
- `source/services/execd/src/` (os-lite restart port, exit-report edge)
- `source/services/samgrd/src/` (staleness + rejects)
- `userspace/nexus-ipc/src/` (Connection re-resolve)
- `source/apps/selftest-client/` (fault-injector phase + probe service)
- `source/apps/selftest-client/proof-manifest/markers/` + `scripts/qemu-test.sh`
- `docs/reliability/` (supervision section), `docs/services/lifecycle.md`

## Plan (small PRs)

1. Spec fields + host tests (pure, no OS risk).
2. samgrd staleness + rejects (host-first).
3. Connection re-resolve (host-first, mock transport).
4. os-lite restart port + exit-report edge + markers.
5. Crash-loop persistence + double-boot proof.
6. Standing fault injector + full gates (`just test-all`).
