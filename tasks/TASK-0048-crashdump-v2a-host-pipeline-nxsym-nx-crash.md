---
title: TASK-0048 Crashdump v2a (host-first): nxsym build-id index + .nxcd format + nx crash CLI + deterministic tests
status: Draft
owner: @reliability
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Crashdumps v1 (baseline): tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Packaging drift note: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
---

## Context

We already planned Crashdumps v1 as a **minimal** approach (in-process capture and host symbolization).
Crashdump v2 aims to turn this into a coherent pipeline with:

- stable dump format,
- fast symbol lookup keyed by Build-ID,
- operator tooling (`nx crash`) to list/show/export/purge,
- deterministic tests without QEMU.

This task is **host-first** by design. OS-side ingestion (`crashd`, VMO artifacts, retention, policy redaction)
is explicitly deferred to `TASK-0049`.

## Goal

Deliver on host:

1. `nxsym` Build-ID → address→line indexer and lookup tool.
2. A compact crash dump container format (`.nxcd` + optional `.zst`) with stable section layout.
3. `nx crash` commands operating on dump directories and fixture dumps.
4. Host tests proving indexer correctness, dump writing/reading, GC/budget logic, and CLI behavior.

## Non-Goals

- Kernel changes.
- OS/QEMU markers.
- Full log/trace correlation (depends on logd/traced, and OS ingestion; see `TASK-0049`).
- Packaging integration into `.nxb/.nxs` (gated; see “red flags”).

## Constraints / invariants

- Deterministic outputs: indexing and dump writing must be stable given the same inputs.
- Bounded memory and bounded output sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (packaging integration drift)**:
  - The repo has known packaging format drift (e.g., `manifest.json` vs `manifest.nxb` direction).
  - v2a will **not** require modifying packers. Indexing can be run on host artifacts as an external tool.
  - Any embedding of symbol indices into bundles is deferred to a dedicated packaging task (or v2b if packaging is stable).

## Stop conditions (Definition of Done)

### Proof — required (host)

- `cargo test -p crashdump_v2_host` green (new).
- `nxsym` tests:
  - Build-ID extraction works (fallback strategy documented and tested).
  - addr2line resolves known frames for a fixture binary.
- `.nxcd` format roundtrip:
  - write → read yields identical header fields and bounded section sizes.
- `nx crash`:
  - `ls/show/export/purge` work on fixture dump directories.

## Touched paths (allowlist)

- `tools/nxsym/` (new)
- `userspace/crash/nxcd/` (new: format crate)
- `tools/nx/` (extend: `nx crash ...`)
- `tests/crashdump_v2_host/` (new)
- `docs/reliability/crashdump-v2.md` (new, host-first sections)
- `docs/devx/nx-cli.md` (extend)

## Plan (small PRs)

1. **`nxsym` tool**
   - Build-ID extraction from `.note.gnu.build-id` with documented fallback.
   - Index file (`symbols.nxsym`) format (CBOR) with stable ordering.
   - CLI:
     - `nxsym index <elf>... -o symbols.nxsym`
     - `nxsym addr2line --sym symbols.nxsym --addr 0x...`

2. **`.nxcd` format crate**
   - Container with named sections:
     - `header.json` (stable keys)
     - `frames.json` (symbolized if available)
     - `maps.json`
     - `logs.jsonl` (optional; bounded)
     - `spans.jsonl` (optional; bounded)
     - `regs.bin` (optional; bounded)
   - Optional zstd wrapper (`.nxcd.zst`) handled outside the core format.

3. **`nx crash` host commands**
   - Operate on a dump directory (default `./crash/` in host tests).
   - `ls/show/export/purge/grep` (host-only for v2a).

4. **Host tests**
   - Build a fixture binary with known frames; index it; verify symbolization.
   - Create fixture dumps; exercise `nx crash`.
   - Budget/GC logic on a directory tree.

