---
title: TASK-0018 Crashdumps & Symbolization v1 (OS): deterministic in-process minidumps + host symbolization + /state export
status: In Review
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md
  - Depends-on (log sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (persistence): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
follow-up-tasks:
  - TASK-0048: Crashdump v2a host pipeline (`nxsym`, `.nxcd`, `nx crash`)
  - TASK-0049: Crashdump v2b OS pipeline (`crashd`, retention, policy redaction)
  - TASK-0141: Crash export/redaction + notifications surface
  - TASK-0142: Problem Reporter UI integration
  - TASK-0227: Offline bugreport bundles (`nx diagnose`)
---

## Context

We want deterministic crash artifacts for userland processes:

- small “minidumps” (regs + tiny memory previews + metadata),
- a stable storage location under `/state/crash/`,
- and a succinct crash event emitted to logd.

The prompt proposes execd capturing registers/stack of a dead child and symbolizing DWARF on-device.
Given the current kernel/userspace ABI, that is not fully feasible without new kernel support.

This repo also already has a v2 crash pipeline direction (`TASK-0048/0049`) and explicitly avoids
format/tool drift. Therefore:

- v1 here is intentionally minimal and does **not** introduce a new “NXMD” format name or a parallel
  `libcrash`/`coredumpd` service plan.
- Offline “bugreport bundle” orchestration is a follow-up (`TASK-0227`) and should reuse the crash
  artifacts from v1/v2 rather than inventing a second container.

## Goal

Provide a v1 crashdump pipeline that works **without kernel changes**:

- processes can emit a deterministic minidump on controlled crash paths (panic/abort), stored under `/state/crash/...`.
- execd emits a crash event to logd containing: pid, exit code, build-id/module id, and the dump path.
- host tooling/tests can symbolize PCs to function/file:line using DWARF for the matching build-id.

## Non-Goals

- Capturing arbitrary crashed process memory/registers from another process (ptrace-like).
- On-device DWARF symbolization for now (too heavy for early OS builds).
- Large dumps, full core files, full backtraces for every fault.

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Determinism**:
  - dump format stable and bounded;
  - no unbounded memory reads; no “best-effort gigabytes”.
- **Privacy/safety**:
  - only bounded stack/code previews;
  - avoid dumping secrets by default (future hardening can add redaction/policy).
- **No fake success**: only emit “minidump written” marker when the write actually succeeded.

## Security considerations

### Threat model

- **Sensitive data exposure**: stack/code previews can leak secrets.
- **Crash-event spoofing**: forged or malformed crash metadata can pollute observability/audit paths.
- **Resource abuse**: oversized dump payloads can exhaust memory/storage in crash paths.

### Security invariants

- Crash artifacts are bounded by strict size caps (header, previews, total frame).
- Dump path stays under `/state/crash/...` with deterministic normalization.
- Crash event metadata originates from trusted process/runtime context, not untrusted payload strings.
- No secret material is logged in markers or crash-event fields.

### DON'T DO

- DON'T add ptrace-like cross-process memory/register capture in this v1 scope.
- DON'T emit `execd: minidump written` or `SELFTEST: minidump ok` before durable write success.
- DON'T dump unbounded stack/code bytes or full process memory.
- DON'T add on-device DWARF symbolization in v1.

## Explicit prerequisites (from TASK-0006 / RFC-0011)

This task assumes:

- `logd` v1 exists and can carry structured crash events emitted by `execd`.
- The crash event envelope keys from RFC-0011 remain stable:
  - required: `event=crash.v1`, `pid`, `code`, `name`, `recent_window_nsec`, `recent_count`
  - reserved for this task to populate: `build_id`, `dump_path`
- `logd` provides bounded `QUERY` so selftests/tools can validate the crash event path without UART scraping.

This task does **not** assume:

- That `execd` can read a dead child’s registers/stack post-mortem without new kernel support (ptrace-like); v1 must be in-process capture.
- Any on-device symbolization or persistent log journaling in logd; symbolization is host-first, persistence is via `/state` (TASK-0009).

## Red flags / decision points

- **RED resolved (contract decision, no blocker for v1)**:
  - **No ptrace/debug syscall surface**: `execd` cannot reliably read dead child regs/stack today.
  - **Decision**: v1 uses **in-process capture/publish only** (no cross-process memory/register scraping; bounded dump bytes come from trusted runtime context).
  - **Escalation path**: cross-process post-mortem capture requires a separate kernel ABI task (out of scope).
- **YELLOW (risky / likely drift / needs follow-up)**:
  - DWARF symbolization on OS may be too heavy initially. v1 should symbolize on host and keep OS dumps as raw PCs + build-id.
  - True “abnormal exits” include page faults; without a kernel-provided trap report interface, only controlled crash paths are covered in v1.
- **GREEN (confirmed assumptions)**:
  - `/state` can host crash artifacts once TASK-0009 lands.
  - logd can carry a structured crash event once TASK-0006 lands.

No unresolved RED items remain for the v1 scope in this task.

## Contract sources (single source of truth)

- Marker contract: `scripts/qemu-test.sh`
- `/state` semantics: TASK-0009
- Crash report marker baseline: TASK-0006 (execd crash report)

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic tests for:
  - minidump encoding/decoding and size caps,
  - symbolization of known PCs against a test ELF/DWARF using build-id mapping,
  - reject paths for malformed/oversized frames and invalid paths.

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `execd: minidump written` (path included)
    - `SELFTEST: minidump ok`
  - Symbolization proof remains host-first in v1; QEMU only proves artifact + event path.

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Required negative tests (Soll-Verhalten)

- `test_reject_oversize_minidump_stack_preview`
- `test_reject_oversize_minidump_total_frame`
- `test_reject_invalid_crash_dump_path_escape`
- `test_reject_unauthenticated_crash_event_publish` (if publish path is exposed)
- `test_reject_malformed_minidump_header`

## Touched paths (allowlist)

- `userspace/crash/` (new: minidump format + writer + optional in-proc backtrace)
- `userspace/statefs/` (client for writing dumps under `/state/crash/...`)
- `source/services/execd/` (emit crash event + “minidump written” marker when a dump is present)
- `source/apps/selftest-client/` (trigger controlled crash and verify markers)
- `userspace/apps/` (add deterministic non-zero crash payload for minidump proof)
- `tools/` (host symbolizer tool / tests)
- `scripts/qemu-test.sh`
- `docs/observability/`

## Implementation status (2026-03-26)

- `userspace/crash` implemented with deterministic v1 framing (`NMD1`), path normalization under `/state/crash/...`, and bounded reject-path tests.
- `execd` crash flow emits structured `crash.v1` fields including `build_id` and `dump_path` when a dump path is available.
- Trusted crash publish path is hardened fail-closed (`test_reject_unauthenticated_crash_event_publish`) and supports deterministic metadata handoff.
- `selftest-client` writes/validates crash dumps and publishes deterministic crash metadata to `execd`, then verifies:
  - `execd: minidump written <path>`
  - `SELFTEST: crash report ok`
  - `SELFTEST: minidump ok`
- Marker honesty hardening: minidump success path now requires read-back + decode verification before `SELFTEST: minidump ok`.
- `execd` validates reported minidump metadata against real persisted dump content before emitting `execd: minidump written`.
- Host symbolization proof added via `tools/minidump-host` with deterministic PC->`fn/file:line` test coverage.
- **Phase 3 complete**: strict child-owned write proof path validated.
  - `demo.minidump` child writes `/state/crash/child.demo.minidump.nmd` itself.
  - `selftest-client` transfers `statefs` caps to the child, then locates and reports the artifact metadata.
  - `statefsd` keeps policy authority and avoids broad bypasses; the v1 proof path is identity-bound and narrowly scoped.

## Phase 3: follow-up drift check (2026-03-26)

Drift was checked against all header follow-up tasks before this phase was documented.

- `TASK-0048` (host pipeline `nxsym`/`.nxcd`/`nx crash`): **no drift**
  - no `.nxcd` or `nxsym` contract introduced in v1 implementation,
  - no host v2 format/tooling scope absorbed.
- `TASK-0049` (OS `crashd` pipeline): **no drift**
  - no `crashd` daemon introduced,
  - no retention/GC or OS symbolization scope absorbed into v1.
- `TASK-0141` (export/redaction/notify): **no drift**
  - no notification/export API introduced here,
  - v1 still focuses on artifact + event proof only.
- `TASK-0142` (Problem Reporter UI): **no drift**
  - no UI work or reporter surface added.
- `TASK-0227` (`nx diagnose` bundles): **no drift**
  - no bugreport bundle/container orchestration added in v1.

### Proof evidence (green)

- `cargo test -p crash -- --nocapture`
- `cargo test -p minidump-host -- --nocapture`
- `cargo test -p execd -- --nocapture`
- `just dep-gate`
- `just diag-os`
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Plan (small PRs)

1. **Minidump format (NMD-lite v1)**
   - Define a compact, deterministic binary format:
     - header: version, ts_nsec, pid, name, build-id/module-id
     - thread snapshot: pc/sp/ra + small fixed register set
     - bounded stack preview (<= 4KiB)
     - bounded code preview (<= 256B around pc) if available
   - Keep encoding simple (fixed-endian structs or a tiny tag-length-value). Avoid heavy codecs in OS builds.

2. **In-process capture path**
   - Provide a small `crashdump` helper that can be invoked from panic/abort paths in OS builds:
     - captures current registers (minimal unsafe, well-documented)
     - writes `/state/crash/<ts>.<pid>.<name>.nmd`
     - emits a deterministic marker on success.

3. **execd integration (metadata + logd event)**
   - On non-zero exit:
     - check if a minidump file exists for the pid/name (or if the child reports the path before exit),
     - emit `execd: minidump written <path>` marker only if present,
     - emit a structured logd event with path + build-id + exit code.

4. **Host symbolization**
   - Implement a host tool/test that:
     - loads an ELF + DWARF,
     - resolves PCs from the dump to fn/file:line,
     - validates deterministic results for a known test binary.
   - OS selftest can validate symbolization indirectly by checking the crash event contains the expected build-id + PC list;
     full fn/file:line can remain host-side for v1.

5. **Selftest**
   - Add a `demo-panic` payload that triggers the in-process dump write.
   - Verify:
     - dump marker observed,
     - execd emits `minidump written`,
     - `SELFTEST: minidump ok`.

## Acceptance criteria (behavioral)

- v1 produces deterministic, bounded crash artifacts without kernel changes.
- Crash event is visible in logd and references the stored dump path.
- Host tests can symbolize a dump against debug info deterministically.
- No unresolved RED decision points remain in this task scope.

## RFC seeds (for later, once green)

- Decisions made:
  - minidump v1 binary format and bounds
  - build-id scheme and resolver mapping
- Open questions:
  - kernel-supported trap/crash report interface for true post-mortem capture
  - on-device symbolization vs host-only pipeline
