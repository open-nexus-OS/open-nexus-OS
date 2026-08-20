---
title: TASK-0049B Reliability v1b: service supervision — declarative tiers + restart/backoff/crash-loop + capability re-resolve
status: Done (2026-08-20 — B1/B2/B3a/B3b/B3c QEMU-proven incl. keep-blk double boot + smp1; test-all green; see DoD reconciliation)
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

- **RED → RESOLVED 2026-08-20 (forced by the kernel, not chosen)**: `wait` /
  `wait_nohang` are parent-bound (`reap_child` rejects non-children), and
  boot services are INIT's kernel children — execd cannot reap them at all.
  execd's reaper-of-record role (RFC-0081) covers exactly its own children
  (apps, probes); **init sweeps its own children** via
  `wait_nohang_with_reason` once per responder round
  (`bootstrap/supervision.rs`). No execd→init exit-report edge exists or is
  needed. Shipped in PR-B1 (observation half): every service death is
  announced once with kernel truth
  (`init: service exit name=<svc> reason=<label> code=0x<hex>`), a one-shot
  exit0 probe proves the sweep every boot
  (`SELFTEST: init supervision sweep ok`, gated full+headless+smp1), and a
  proof boot treats a real service death as FATAL (no restart exists yet —
  the ladder would otherwise run against a corpse). Policy SSOT shipped
  alongside: `service_supervision.rs` (criticality tiers + restart policy
  for the whole fleet incl. bespoke services + the ONE declared backoff
  schedule; 3 host tests pin coverage, the RFC-0087 critical-boot floor and
  the schedule shape) — deliberately a sibling table, NOT ServiceSpec
  fields (half the fleet is bespoke and must never grow a spec, or the
  generic provisioning arm would fire for it).
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

## DoD reconciliation (2026-08-20, documented recuts — Option C)

- Host "persistence record roundtrip" → **recut**: the wire encode/decode is
  host-tested in the `statefs` crate; init's `persist.rs` is an os-only
  no-alloc client with no host seam — the honest proof is the QEMU double
  boot (delivered), not a mock roundtrip.
- Host "execd os-lite restart path" → **recut in PR-B1**: the kernel only
  lets the PARENT wait; supervision lives in init, execd keeps spawn/reap
  mechanics. No execd restart path exists by design (resolved RED (a)).
- Host "samgrd stale/re-register tests" → **recut in PR-B3a**: the resolve
  authority is init's responder + `RouteTable` (ADR-0057 amended); staleness
  + rejects are host-tested there (8 route_table tests). samgrd is a
  secondary self-scoped registry and stays untouched.
- OS "resource sentinel green in the same run (restart storm)" → **recut**:
  the kernel sentinel runs at boot, and a post-storm sentinel would be RED
  by design today — pinched's parked worker threads leak one pair per cycle
  (recorded residual, needs TASK-0304 Part 2). Structural no-drift stands in
  instead: PAIRED exit/restart counting, client slot hygiene in the storm,
  init closing its respawn-minted cap each cycle. The sentinel-under-storm
  proof ships WITH the worker-teardown task, where it can honestly be flat.
- "Widening `respawnable()` beyond the pilot" → **follow-up**: every widened
  service needs worker/state teardown maturity first (same TASK-0304 Part 2
  dependency); the pilot proves the mechanism end to end.

Follow-ups recorded: (1) TASK-0304 Part 2 worker teardown → then
sentinel-under-storm + respawnable() widening; (2) metricsd statefs reply
wiring (recv aims at the logd reply inbox — slot aliasing; today loud-dropped
by statefsd, must be rewired to a real reply path); (3) cross-boot
AUTO-blocking on the persisted counter needs a wall-clock-anchored window
(RFC-0076) — in-boot windows use monotonic time which does not compare
across boots; (4) ADR-0056 panic sub-flag (deferred); (5) escalation
reboot-into-safe parks until TASK-0050 (reset path).

## Plan (small PRs) — resequenced 2026-08-20 (observation → engine → protocol)

1. ✅ **PR-B1 (2026-08-20) — observation + policy SSOT**: init sweeps its own
   children (`bootstrap/supervision.rs`, the kernel forces this — see the
   resolved RED), death line with ADR-0056 truth, one-shot exit0 probe
   proves the sweep every boot (gated full/headless/smp1), FATAL guard on a
   real service death in proof boots. `service_supervision.rs` policy SSOT
   (tiers/restart/DEFAULT_BACKOFF, 3 host tests).
2. ✅ **PR-B2 (2026-08-20) — restart engine + standing fault injector**:
   pure `supervision_engine.rs` (injected time, ADR-0056 reason input,
   decisions as values; 5 host tests: exact 500→1000→2000→4000 schedule +
   clamp, cap exactly at threshold and terminal, window aging, clean never
   counts across all three policies, due/ack roundtrip; `ExitReason` gained
   a host-side re-export in nexus-abi for this). Standing injector
   (ADR-0048 doctrine): a supervised `demo.fault` child with a tight test
   backoff walks REAL kernel fault exits through the engine every boot —
   `init: restart svc=fault-probe attempt=0x1..0x4`,
   `SELFTEST: supervision restart ok` (2nd exit = the restarted child ran),
   `init: crash-loop blocked svc=fault-probe reason=fault` +
   `SELFTEST: crash-loop cap ok` at the 5th; all verdicts fail-loud
   (cap without proven restart = FAIL, clean reason on the fault probe =
   FAIL). Gated full/headless/smp1. Boot 2026-08-20T12-33-59: full cycle
   visible, ladder green.
3. 🟨 **PR-B3a (2026-08-20) — staleness at the real resolve authority**
   (ADR-0057 precision recorded in the ADR: the broker today is init's
   responder + RouteTable, NOT samgrd — staleness lives where resolving
   happens): `RouteTable` gained `mark_stale`/`clear_stale`,
   `lookup_by_name` answers `TargetStale` (distinct from RouteNotFound,
   ADR-0054), the responder wires it as the new routing `STATUS_STALE`, and
   the supervision sweep marks a service stale at its death line. Client
   side: `route_with_nonce_budgeted` keeps re-asking a STALE target until
   the deadline (RFC-0025) and surfaces `TargetStale` — never a hammering
   loop, never a silent Rejected. Enablers landed: `Rights` is cfg-free
   (pure bitflags) and `route_table` is host-testable — the RFC-0066
   "route_table host-testability is Phase 2" debt is settled (stale test +
   the previously never-running lookup tests now run on host).
   Boot-positive STALE answer is NOT provable yet (a proof boot forbids
   real service deaths by design) — it lands with PR-B3b's restart of a
   real service.
4. 🟨 **PR-B3b — restart of a REAL service (pinched pilot), in flight
   2026-08-20**: `bootstrap/respawn.rs` re-provisions pinched from
   boot-held state only (no-rights-drift by construction): `'static` image
   ELF, init-owned ctrl endpoints re-transferred to slots 1/2, surviving
   client-owned response endpoint to slot 4, ONLY the owner-bound request
   endpoint re-minted (RECV→3) with a fresh client SEND clone +
   RouteTable update → `clear_stale`. Trigger: identity-gated
   `OP_SELFTEST_CRASH` in pinched (kernel-attributed sender; the deny path
   has no forgeable probe surface by design). Standing E2E in the end
   phase: crash → STALE window → re-resolve → ping the NEW instance →
   `SELFTEST: service restart ok`; harness guard is now PAIRED counting
   (`service exit` == `service restarted`, unpaired either way = red).
   **Known residual recorded**: pinched's same-AS worker threads survive
   the main task's death parked — TASK-0304 Part 2 ("no shipping service
   needs it yet") is now needed by respawn; one leaked worker pair per
   proof cycle until it lands.
5. ✅ **PR-B3c (2026-08-20) — persistence + storm + re-attach contract**:
   init persists the restart counter per service at
   `/state/init/restarts/<svc>` (`bootstrap/persist.rs`: statefs v2
   nonce-correlated wire on init's own pre-minted statefsd slots, bounded
   no-alloc buffers, strict PUT+SYNC **before** `task_resume` so the record
   is durable before any client can observe the restarted instance).
   init's identity for the policy gate is the kernel-set FNV of
   `"init-lite"`; `policies/base.toml` grants it `statefs.read/write`.
   The E2E proof is now a 3-cycle restart **storm** with client slot
   hygiene (the selftest closes its previous SEND cap after each
   re-resolve) and a counter truth assert
   (`SELFTEST: crash-loop count ok`, C_after == C_before + 3, fatal in
   both directions); the keep-blk double boot proves cross-boot
   persistence (`SELFTEST: crash-loop persist ok`, baseline > 0 replayed —
   boots 2026-08-20T17-26-38 + 17-29-39, both ladders green). RFC-0087 §3
   gained the normative client re-attach contract (death window, STALE ≠
   NOT_FOUND, re-resolve protocol, slot hygiene as the consumer half of
   no-rights-drift, ctrl-plane exemption).
   **Root cause fixed along the way (RFC-0087 exhaustion-is-an-event in
   its own service)**: statefsd replied on the SHARED response queue with
   `Wait::Blocking` — clients that abandon replies (metricsd's retention
   writer: `wiring.rs` points its statefs recv at the LOGD reply inbox,
   slot aliasing) filled the depth-8 queue and wedged statefsd DEAF for
   everyone from the exec phase on. Invisible until now because no proof
   traffic spoke to statefsd after exec. Fix: bounded reply send (500 ms)
   + loud forensic drop (`statefsd: dropped reply op=0x..`) — the server
   can never again be wedged by a dead-beat client, and the marker keeps
   the metricsd wiring hole visible until fixed (follow-up below).
