---
title: TASK-0038 Tracing v2: cross-node correlation via DSoftBus (context propagation + sampling + traced collector) (rebased 2026-08-14 onto shipped tracing v1 + real OS mux)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0006
  - TASK-0009
  - TASK-0014
  - TASK-0020
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Depends-on (local spans/metrics baseline): tasks/TASK-0014-observability-v2-metrics-tracing.md
  - Depends-on (log sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (persistence for JSONL): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (DSoftBus mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (OS DSoftBus networking): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Time sync service (placeholder today): source/services/time-syncd/
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0014 covers **local** spans/metrics and exporting span end events via logd. This task extends tracing
to a distributed system:

- stable TraceId/SpanId and trace flags,
- context propagation across process boundaries and across nodes (DSoftBus streams),
- adaptive sampling and privacy rules,
- a collector/ingester (`traced`) that correlates and aligns time across nodes.

## Rebase note (2026-08-14) — verified against repo reality

**The original gating premise is inverted.** The ledger claimed "OS DSoftBus backend is still a
placeholder until networking tasks land" — that is stale: TASK-0003 and TASK-0005 are Done, and
dsoftbusd's OS path runs a real mux with authenticated sessions today
(`source/services/dsoftbusd/src/os/session/cross_vm.rs:21-24` imports the OS-side mux_v2 module).
The 2-VM lane markers `dsoftbus:mux session up` / `dsoftbus:mux data ok` are gated in
`scripts/qemu-test.sh` (REQUIRE_DSOFTBUS ladder, `:1354-1355`).

**Corrected integration target (touched paths fixed below):** the mux to extend is the mux_v2
module compiled into dsoftbusd — SSOT file `userspace/dsoftbus/src/mux_v2.rs`, included OS-side via
`#[path]` at `source/services/dsoftbusd/src/os/mod.rs:23` as `crate::os::mux_v2`. It is
**NOT** the `userspace/dsoftbus` OS backend shim: `userspace/dsoftbus/src/os.rs` is
`STATUS: Placeholder` (panicking stubs) and is deliberately bypassed by dsoftbusd.

**What already shipped via TASK-0014 (Done) — do NOT re-implement:**

- `userspace/nexus-metrics/src/lib.rs` already defines the tracing SSOT:
  `TraceId(pub u64)` (`:80`), `SpanId(pub u64)` (`:75`), `SpanGuard` (`:190`), and deterministic
  `next_trace_id()` (`:164`, per-service seeded, no RNG).
- Span-end export to logd is proven: `SELFTEST: tracing spans ok` is gated at
  `scripts/qemu-test.sh:566`.
- `docs/observability/tracing.md:20` explicitly leaves cross-node propagation/correlation to
  follow-up work — this task's scope is valid and unclaimed.

**Open contract decision (record, do not silently resolve):** this ledger originally wanted a
128-bit TraceId; the shipped SSOT is `TraceId(pub u64)`. v2 keeps **u64** unless the RFC seed that
this task requires for the propagation wire format (new cross-node protocol → RFC first, per
CLAUDE.md workflow) decides otherwise. Do not fork a second TraceId type.

**Hard gate on cross-VM proof:** the 2-VM lane is red
(`tasks/TRACK-NETWORK-PROOF-LANES.md`, `OS2VM_E_DISCOVERY_TIMEOUT`); cross-node OS markers are
blocked until that track's repair lands. Host-first propagation/correlation work can proceed now.

Repo reality today (corrected):

- OS DSoftBus mux + authenticated sessions are real (TASK-0003/0005 Done); the remaining OS gate is
  the proof-lane repair, not missing networking code.
- Mux v2 baseline from `TASK-0020` is implemented and runs in the OS build.
- `/state` persistence exists (statefs v1 shipped, TASK-0009 Done).

Therefore this task is **host-first**, with cross-VM OS markers gated on the proof-lane repair —
honest markers only when real behavior exists.

## Goal

Deliver cross-node tracing v2 such that:

- services can create spans with stable ids,
- trace context can be propagated over DSoftBus streams (mux headers),
- traced correlates events by TraceId and writes JSONL (and optionally exports OTLP/HTTP, disabled by default),
- host tests prove propagation + correlation deterministically.

## Non-Goals

- Full OpenTelemetry compliance.
- Tail-based sampling in v2 (possible later).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic id generation in OS builds (no RNG dependency unless we provide an explicit entropy source).
- Bounded memory:
  - cap baggage entries,
  - cap live span table,
  - cap “active traces” in traced (LRU).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: “cross-node ok” only after a real remote edge was observed and correlated.

## Red flags / decision points

- **RED (gating)**:
  - Cross-node OS proof depends on the `tasks/TRACK-NETWORK-PROOF-LANES.md` repair (2-VM lane red
    in `OS2VM_E_DISCOVERY_TIMEOUT`); the mux itself is real (see rebase note).
- **YELLOW (clock alignment)**:
  - `time-syncd` is currently a placeholder. v2 must support “best effort” alignment:
    - host tests use an injected clock/offset,
    - OS uses a simple offset provider once time-syncd becomes real.
- **YELLOW (privacy)**:
  - baggage must have a “private:*” convention and must not cross trust boundaries by default.

## Contract sources (single source of truth)

- DSoftBus mux contract: `userspace/dsoftbus/src/mux_v2.rs` (SSOT; compiled into dsoftbusd via
  `#[path]` at `source/services/dsoftbusd/src/os/mod.rs:23`).
- Tracing id/span SSOT: `userspace/nexus-metrics/src/lib.rs` (TASK-0014, Done).
- log sink semantics: TASK-0006
- marker contract: `scripts/qemu-test.sh` (OS-gated)

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/tracing_v2_host/`):

- inject/extract context (compact binary + text-map)
- propagate parent→child across a mock IPC boundary
- cross-node: two in-proc DSoftBus mux sessions exchange a request; traced correlates by TraceId and writes JSONL
- sampling: parent-based sampling is honored; forced-sample list works
- privacy: “private:*” baggage keys are dropped on cross-node injection.

### Proof (OS / QEMU) — gated

Once the 2-VM proof lane is green (TRACK-NETWORK-PROOF-LANES; mux and `/state` already exist):

- `traced: ready`
- `dsoftbus: mux trace caps on`
- `dsoftbus: mux trace propagated`
- `traced: cross-node ok`
- `SELFTEST: mux trace ok`
- `SELFTEST: trace jsonl ok`

## Touched paths (allowlist)

- `userspace/telemetry/` (new trace lib — thin layer over the `nexus-metrics` id SSOT, not a fork)
- `source/services/traced/` (new collector)
- `userspace/dsoftbus/src/mux_v2.rs` (mux trace metadata extension; SSOT compiled into dsoftbusd
  as `crate::os::mux_v2` — NOT `userspace/dsoftbus/src/os.rs`, which is a bypassed placeholder)
- `source/services/dsoftbusd/` (OS mux integration + markers)
- `source/apps/selftest-client/` (OS-gated)
- `tests/` (host tests)
- `docs/observability/tracing.md`
- `scripts/qemu-test.sh` (OS-gated)

## Plan (small PRs)

1. **Core trace context library (`nexus-trace`)**
   - Reuse the shipped SSOT: `TraceId(u64)` / `SpanId(u64)` from `nexus-metrics` (128-bit only if
     the propagation RFC decides so — see rebase note); flags, bounded baggage.
   - Deterministic id generation already exists (`next_trace_id()`); extend, don't duplicate.
   - Inject/extract into:
     - compact binary header (for mux),
     - text-map (for debugging/tools).

2. **Propagation**
   - DSoftBus mux: attach trace metadata to stream open on top of the existing mux v2 baseline.
   - Optional IPC propagation: provide helpers for services to attach trace meta to their byte frames
     (do not require kernel changes or universal adoption in v2).

3. **Collector (`traced`)**
   - Ingest spans/events via:
     - direct IPC from services (preferred),
     - and/or logd records (fallback) once logd exists.
   - Correlate by TraceId; write JSONL to `/state/trace/*.jsonl` (gated on statefs).
   - Optional OTLP/HTTP exporter disabled by default (feature flag + runtime switch).

4. **Sampling policy**
   - ParentBased sampler with default rate (e.g., 10% roots).
   - Force-sample allowlist by span name.
   - Drop private baggage keys on cross-node propagation.

5. **Docs**
   - Trace model, propagation rules, sampling and privacy.
