---
title: TASK-0049 Crashdump v2b (OS): crashd ingestion + VMO artifacts + retention/GC + log/trace correlation + policy redaction
status: Draft
owner: @reliability
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Crashdump v2a (host tools + format): tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md
  - Crashdumps v1 baseline: tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Observability v1 (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Tracing correlation (future): tasks/TASK-0038-tracing-v2-cross-node-correlation.md
  - Policy as Code (redaction + export gates): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (reload + 2PC): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Kernel remains untouched; therefore:

- `execd` cannot reliably read registers/stack of another process unless the process itself records them.
- Crash capture must be **in-process** (panic/abort hook) and forwarded best-effort to a crash pipeline.

This task delivers the OS-side “crash pipeline”:

- `nexus-crash` in-process capture,
- `crashd` ingestion and storage under `/state/crash`,
- retention/GC budgets,
- correlation with recent logs (and later traces),
- policy-based redaction and export gating.

## Goal

On OS/QEMU, provide:

1. `nexus-crash` runtime hook to emit `CrashRecord` + optional VMO attachments (bounded).
2. `crashd` service that ingests, symbolizes (via packaged symbols), writes `.nxcd.zst` + `.report.txt`.
3. Retention/GC (TTL + disk budget).
4. Optional redaction and export gates driven by Policy as Code (default: export disabled).
5. Markers and selftest proof with controlled crash.

## Non-Goals

- Kernel changes.
- Perfect “ptrace-like” minidumps (not possible without kernel debug API).
- Cross-node correlation (separate task).

## Constraints / invariants

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded crash path:
  - crash hook must be best-effort and must not deadlock the process.
  - crashd must cap logs/spans/stack bytes.
- Deterministic markers for `scripts/qemu-test.sh`.

## Red flags / decision points

- **RED (OS gating dependencies)**:
  - Needs `/state` persistence (`TASK-0009`) to store crash artifacts.
  - Needs `logd` (`TASK-0006`) to pull “recent logs”.
  - Needs a stable symbol distribution story (either packaging embedding or sidecar index).
  - If those are not present, OS proof is blocked; implement host-first v2a first.
- **YELLOW (VMO attachments)**:
  - VMO transfer is planned and may be available via capability transfer, but must be proven in QEMU before relying on it.
  - v2b must have a filebuffer fallback if VMO is unavailable.
- **YELLOW (policy redaction)**:
  - Policy must default to conservative redaction.
  - Never include obvious secret paths (e.g., `/state/secret/*`) regardless of policy.

## Stop conditions (Definition of Done)

### Proof — required (OS/QEMU) once gated deps exist

UART markers:

- `crashd: ready`
- `execd: crash detected`
- `crashd: dump written (id=... bytes=...)`
- `SELFTEST: crash symbolized ok`
- `SELFTEST: crash gc ok`

## Touched paths (allowlist)

- `userspace/runtime/nexus-crash/` (new)
- `source/services/crashd/` (new)
- `source/services/execd/` (wire crash hook markers; best-effort fallback)
- `docs/reliability/crashdump-v2.md` (OS sections)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-crashdump.sh` (delegating to canonical proofs)
- `scripts/qemu-test.sh` (marker list)

## Plan (small PRs)

1. **`nexus-crash` runtime hook**
   - alt-stack and panic hook for controlled capture (best-effort).
   - emit `CrashRecord` + optional bounded attachments to crashd.

2. **`crashd` service**
   - ingest `CrashRecord`
   - symbolize using `nxsym` indices if available (fallback to raw PCs)
   - pull recent logs from `logd` (bounded)
   - write `/state/crash/<date>/<id>.nxcd.zst` and `<id>.report.txt`
   - markers: `crashd: ready`, `crashd: dump written ...`

3. **Retention/GC**
   - TTL + `max_bytes` budget
   - LRU by last-access or creation time (documented)
   - marker `crashd: retention gc on` once per boot

4. **Policy redaction + export gating**
   - integrate `policyd` evaluation (v1 rules) for:
     - allowed attachments (none/stack-only/full)
     - export enable + destination allowlist

5. **Selftest + postflight**
   - controlled crash app with known 3-frame stack
   - verify symbol name appears in report
   - force GC in test mode and verify marker

## Follow-ups

- Crash notifications + export/redaction surface (single-report export): `TASK-0141`
- Problem Reporter UI (view/delete/export): `TASK-0142`
