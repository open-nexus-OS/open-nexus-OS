---
title: TASK-0048 Crashdump v2a (host-first): nxsym build-id index + .nxcd format + nx crash CLI + deterministic tests
status: Done
owner: @reliability
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
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
- `docs/dev/nx-cli.md` (extend)

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

## Progress

### 2026-08-14 — v2a implemented and closed (host proofs complete)

Reuse acknowledgment: TASK-0018 (Done) already shipped minidump v1 —
`userspace/crash` provides `MinidumpFrame` **with an embedded `build_id`**
(execd stamps `crash::deterministic_build_id(name)`), plus host symbolizer
prior art in `tools/minidump-host`. v2a builds on that instead of reinventing:
`nxsym` **consumes** `MinidumpFrame.build_id` verbatim as the lookup key
(carried through `.nxcd` `header.json`/`frames.json`/`maps.json`), and the
documented indexing fallback for ELFs without a `.note.gnu.build-id` note is
the very same `crash::deterministic_build_id(<file stem>)` so index keys and
dump keys can never drift.

Shipped:

- `userspace/crash/nxcd/` — `.nxcd` container crate: canonical binary layout
  (kind-sorted table, contiguous payloads, per-section + 1 MiB total bounds),
  typed JSON sections with stable keys, minidump-v1 conversion, pure GC
  planner, zstd wrapper behind host-only `zst` feature (bomb-bounded decode).
- `tools/nxsym/` — lib + CLI: GNU-note Build-ID extraction w/ tested fallback,
  CBOR `symbols.nxsym` (stable ordering, bounded + invariant-validated reads),
  `index` / `addr2line` commands, Build-ID keyed lookup.
- `tools/nx` — new `nx crash ls|show|export|purge|grep` subcommand
  (`src/commands/crash.rs`), `--sym` symbolization in `show`, canonical
  `.nxcd.zst` export, deterministic purge via `nxcd::plan_purge`.
- `tests/crashdump_v2_host/` — end-to-end host suite (real fixture binary
  indexing + symbolization, file roundtrips, GC on a directory tree, rejects).
- Docs: `docs/reliability/crashdump-v2.md` (new), `docs/dev/nx-cli.md`
  (crash commands; `nx crash` removed from "future topics").
- Red flag honored: no packer was touched; packaging embedding stays deferred.
- Dependency note: `zstd` (C binding) enters host tools only; `jobserver`
  pinned to 0.1.33 in `Cargo.lock` to keep the `r-efi` duplicate ban green;
  `just dep-gate` confirms the OS graph stays clean.

### Closure — DoD met by host proofs (this task is host-only by design)

- `cargo test -p crashdump_v2_host` → ok, 8 passed (indexer correctness,
  Build-ID keyed symbolization of a known frame incl. file+line, `.nxcd`
  plain/zst file roundtrips, GC/budget on a directory tree, reject paths).
- `cargo test -p nxcd --features zst` → ok, 19 passed (roundtrip identical
  header fields + bounded sections, `test_reject_*` for magic/version/
  truncation/order/oversize/bomb).
- `cargo test -p nxsym` → ok, 12 lib + 3 CLI passed (GNU-note extraction on a
  synthetic ELF, fallback == producer derivation, index roundtrip/rejects,
  process-boundary `index`/`addr2line` on a real binary).
- `cargo test -p nx` → ok, all suites green incl. new `tests/crash_cli.rs`
  (8 passed: ls/show/export/purge/grep + symbolized show + rejects).
- Gates: `just structure-gate` PASS, `just dep-gate` PASS,
  `just deny-check` PASS (advisories/bans/licenses/sources ok), clippy
  `-D warnings` clean on nxcd/nxsym/nx, approved rustfmt clean.
