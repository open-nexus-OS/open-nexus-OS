---
title: TASK-0006 Observability v1 (OS): logd journal + nexus-log client sink + execd crash reports
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0003-unified-logging.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We already have bring-up oriented UART logging and a unified facade (`source/libs/nexus-log`), but:

- `source/services/logd` is currently a placeholder and does not provide an OS journaling service.
- OS services still rely on ad-hoc prints/markers for observability; we want structured logs with bounded memory.
- We want minimal, honest crash reporting: when a spawned process exits non-zero, capture recent logs + metadata and emit a deterministic crash marker.

This task adds **Observability v1** entirely in userspace with **kernel unchanged**.

## Goal

In QEMU, prove:

- `logd` runs as a service, maintains a bounded RAM journal, and (optionally) mirrors INFO+ to UART in a deterministic format.
- A client API is available to services/apps so they can emit structured records to `logd` (without `println!` in services).
- `execd` emits a crash report marker for a controlled non-zero exit and includes a small summary derived from recent log records.

## Non-Goals

- Kernel logging changes, kernel ring buffers, or kernel syscalls for logging.
- Full “observability control plane” (routing, subscriptions, persistence, remote export).
- Full crash dumps / backtraces / symbolization (only a minimal v1 envelope).

## Future direction (not in v1): “logging deluxe” target (v2+)

We *do* want a richer logging system long-term (buffers, routing, persistence, tooling), but we
explicitly keep it out of v1. This section exists to ensure v1 is constructed in a way that does
not paint us into a corner.

Deluxe goals (v2+ examples, not exhaustive):

- Persistent journal (statefs/blk-backed) with rotation and size quotas.
- Subscriptions / streaming (log consumers) with backpressure.
- Per-target/topic runtime filters (`logctl`) and structured fields (schema evolution).
- Export/bridging (e.g. to a remote collector via `dsoftbusd`) and optional tracing/metrics.
- Capability-gated access control for query/subscribe (policy audited).

v1 design constraints to stay compatible:

- Keep record encoding versioned and bounded (size caps; deterministic parsing).
- Keep the API surface small but extensible (Append/Query/Stats now; Subscribe later).
- Avoid embedding host-only assumptions (no reliance on `std::net`, filesystem persistence, etc.).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **No fake success**: readiness/selftest markers must only appear after real behavior.
- **Bounded memory**:
  - journal capacity is fixed (records/bytes) and overflow drops oldest with a `dropped` counter.
  - record size is capped; oversized records are rejected deterministically.
- **Determinism**:
  - markers are stable strings; UART mirror output is deterministic (no timestamps in marker lines).
  - time-based logic (sinceNsec) must be robust if clocks are coarse.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - On-wire contract choice: OS-lite currently prefers small, versioned byte frames. If we want Cap’n Proto to be the *only* contract for logd, we must explicitly decide and prove it works in OS builds; otherwise keep Cap’n Proto as “schema/docs” and make byte frames authoritative for v1.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - Startup ordering: services may log before `logd` is ready. We must choose and document a bounded fallback (UART-only vs small local buffer) and ensure it cannot emit misleading “logged” success.
  - “Structured fields” encoding: JSON vs CBOR affects determinism/size; keep it opaque in v1 and cap sizes.
- **YELLOW (needs design clarification before implementation)**:
  - Execd supervision hook: the current os-lite `execd` path spawns via kernel `exec` and returns a pid, but the “child exited pid=… code=…” marker in QEMU logs is produced by `selftest-client` using the `wait()` syscall. For crash reports, `execd` must either:
    - actively supervise spawned pids (poll `wait()` in its own loop), or
    - delegate supervision to init/selftest and only provide a “crash report” RPC.
    This choice affects where the crash report marker is emitted and how we correlate logs to a pid.
- **GREEN (confirmed assumptions)**:
  - `source/libs/nexus-log` already exists as the unified facade (RFC-0003) and is the right place to attach a logd sink.
  - Kernel provides a `wait()` syscall used in `selftest-client`, so userspace can observe exits without kernel changes.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **Logging facade**: `source/libs/nexus-log` (must remain the only “default” logging API in OS code)
- **Service protocols**:
  - For OS bring-up we prefer compact, versioned byte frames (RFC-0005 style) unless we explicitly decide to move to Cap’n Proto.
  - IDL schemas (Cap’n Proto) may be added as *documentation + future direction*, but must not become the only on-wire contract unless proven in OS builds.

## Stop conditions (Definition of Done)

### Proof (Host)

- Add/extend deterministic unit/integration tests under the existing crates (avoid inventing a parallel test harness unless necessary):
  - `cargo test -p logd -- --nocapture` (ring behavior: order, drop, stats)
  - `cargo test -p nexus-log -- --nocapture` (client formatting / backend selection)

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend `scripts/qemu-test.sh` expected markers (order tolerant) with:
    - `logd: ready`
    - `SELFTEST: log query ok`
    - `execd: crash report` (service/pid/code visible)
    - `SELFTEST: crash report ok`

## Touched paths (allowlist)

- `source/services/logd/` (implement OS journaling service + marker)
- `source/libs/nexus-log/` (add a client sink that can forward to logd; keep UART fallback)
- `source/services/execd/` (crash-report hook on non-zero exit; query logd)
- `source/apps/selftest-client/` (log query + controlled crash markers)
- `userspace/apps/` (add a tiny deterministic crash payload app, e.g. exit code 42)
- `tools/nexus-idl/schemas/` (optional: add `log.capnp` as schema documentation)
- `scripts/qemu-test.sh` (canonical contract update)
- `docs/` (logging guide + testing notes)

## Plan (small PRs)

1. **Define the on-wire contract (OS-first)**
   - Decide the minimal v1 OS wire protocol for logd:
     - `LOG` magic + versioned ops: APPEND / QUERY / STATS.
     - bounded strings (scope/msg), bounded opaque fields blob.
   - (Optional) Add `tools/nexus-idl/schemas/log.capnp` as a future-facing schema, but keep the OS contract authoritative and testable.

2. **Implement `logd` (RAM ring journal + optional UART mirror)**
   - Bounded ring buffer (records or bytes), drop-oldest policy, `total` + `dropped` counters.
   - IPC handler over the existing kernel IPC fabric (no kernel changes):
     - APPEND validates record sizes and stores into ring.
     - QUERY returns records since a timestamp (or best-effort fallback if timestamps unavailable).
     - STATS returns total/dropped/capacity.
   - UART mirror (optional, build/env controlled): INFO+ to UART as JSONL (single-line, bounded).
   - Emit readiness marker: `logd: ready` only after IPC endpoints are live.

3. **Extend `nexus-log` to support logd as a sink**
   - Keep existing deterministic UART marker behavior (fallback).
   - Add a logd sink backend:
     - OS: send APPEND frames to logd via kernel IPC.
     - Host: in-proc or loopback backend for tests.
   - Ensure services can initialize even if logd is not ready:
     - bounded local buffering or UART-only fallback (explicit; no “silent ok”).

4. **Wire core services**
   - Replace ad-hoc `println!` paths in services/apps with `nexus-log` calls (structured logs), while preserving the existing UART readiness markers required by `scripts/qemu-test.sh`.

5. **Crash reporting in `execd`**
   - On child exit with code != 0:
     - query last N records/seconds from logd (bounded),
     - emit a single crash report marker + a structured log record with summary fields.
   - Marker:
     - `execd: crash report <service> pid=<pid> code=<code>`

6. **Selftest proof**
   - Emit some `nexus-log` records from selftest scope.
   - Query logd and verify at least one record matches; marker: `SELFTEST: log query ok`.
   - Run a controlled crash payload (exit code 42); wait for execd crash report marker; marker: `SELFTEST: crash report ok`.

7. **Docs**
   - Add `docs/observability/logging.md`:
     - logd ring design, bounds, UART mirror semantics
     - nexus-log usage patterns (startup ordering + fallback)
     - crash reporting flow (v1 limits)
   - Update `docs/testing/index.md` with marker expectations and troubleshooting.

## Acceptance criteria (behavioral)

- Host tests prove ring ordering, drop behavior, and stats deterministically.
- OS/QEMU run produces required markers and no kernel changes.
- Services use `nexus-log` (no new `println!` in OS daemons) while preserving the existing readiness markers.

## Evidence (to paste into PR)

- Host: `cargo test -p logd` and `cargo test -p nexus-log` summaries
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` and `uart.log` tail with:
  - `logd: ready`
  - `SELFTEST: log query ok`
  - `execd: crash report ... code=42`
  - `SELFTEST: crash report ok`

## RFC seeds (for later, once green)

- Decisions made:
  - OS wire protocol for logd (frames, bounds, versioning).
  - nexus-log sink selection + fallback policy.
  - crash report envelope shape and limits.
- Open questions:
  - persistence (flash journal) and logctl tooling
  - structured fields encoding (CBOR vs JSON) and schema evolution
