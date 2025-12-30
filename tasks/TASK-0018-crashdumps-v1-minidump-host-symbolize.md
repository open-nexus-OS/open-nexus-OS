---
title: TASK-0018 Crashdumps & Symbolization v1 (OS): deterministic in-process minidumps + host symbolization + /state export
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (log sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (persistence): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
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

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - **No ptrace/debug syscall surface**: today, `execd` cannot reliably read a child’s registers/stack after it dies.
    The only safe, kernel-unchanged v1 is **in-process capture** (the crashing process writes its own dump).
    If we require “execd captures dead child regs/stack”, that becomes a kernel ABI task.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - DWARF symbolization on OS may be too heavy initially. v1 should symbolize on host and keep OS dumps as raw PCs + build-id.
  - True “abnormal exits” include page faults; without a kernel-provided trap report interface, only controlled crash paths are covered in v1.
- **GREEN (confirmed assumptions)**:
  - `/state` can host crash artifacts once TASK-0009 lands.
  - logd can carry a structured crash event once TASK-0006 lands.

## Contract sources (single source of truth)

- Marker contract: `scripts/qemu-test.sh`
- `/state` semantics: TASK-0009
- Crash report marker baseline: TASK-0006 (execd crash report)

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic tests for:
  - minidump encoding/decoding and size caps,
  - symbolization of known PCs against a test ELF/DWARF using build-id mapping.

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `execd: minidump written` (path included)
    - `SELFTEST: minidump ok`
    - `SELFTEST: symbolize ok` (symbolization validated via logd or via a deterministic host-side check described below)

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Touched paths (allowlist)

- `userspace/crash/` (new: minidump format + writer + optional in-proc backtrace)
- `userspace/statefs/` (client for writing dumps under `/state/crash/...`)
- `source/services/execd/` (emit crash event + “minidump written” marker when a dump is present)
- `source/apps/selftest-client/` (trigger controlled crash and verify markers)
- `userspace/apps/` (add `demo-panic` / `demo-abort` payload)
- `tools/` (host symbolizer tool / tests)
- `scripts/qemu-test.sh`
- `docs/observability/`

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

## RFC seeds (for later, once green)

- Decisions made:
  - minidump v1 binary format and bounds
  - build-id scheme and resolver mapping
- Open questions:
  - kernel-supported trap/crash report interface for true post-mortem capture
  - on-device symbolization vs host-only pipeline
