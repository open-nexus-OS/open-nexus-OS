<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Crashdump v2 (host-first pipeline)

Status: v2a shipped (host); v2b at-rest writer shipped (TASK-0051B — execd-side, NO `crashd` daemon).

## CONTEXT

- Scope: `TASK-0048` — `.nxcd` container format, `nxsym` Build-ID symbol index,
  `nx crash` operator commands, deterministic host tests.
- Baseline: minidump v1 (`TASK-0018`, RFC-0031) — `userspace/crash` frames with
  an embedded `build_id`; host symbolizer prior art in `tools/minidump-host`.
- Canonical artifact: `.nxcd.zst` (registered in `tasks/TRACK-AUTHORITY-NAMING.md`;
  no parallel dump formats).
- Proof commands: `cargo test -p crashdump_v2_host`, `cargo test -p nxsym`,
  `cargo test -p nxcd --features zst`, `cargo test -p nx` (`tests/crash_cli.rs`).

## Pipeline overview

```
producer (OS, v1)          host tooling (v2a)
------------------         ---------------------------------------------
execd writes .nmd   --->   nx crash export  --->  .nxcd.zst (canonical)
(MinidumpFrame with        nx crash ls/show/purge/grep over dump dirs
 build_id embedded)        nxsym index <elf>... -o symbols.nxsym
                           nx crash show --sym / nxsym addr2line
```

The `build_id` in every crash artifact originates from the producer
(`MinidumpFrame.build_id`) and is carried through conversion verbatim.
Symbolization keys on it; **host tools never re-derive Build-IDs for dumps**.

## `.nxcd` container format (v1)

Single-file binary container with named, bounded sections.

Layout (all integers little-endian):

| offset | field | size |
| --- | --- | --- |
| 0 | magic `NXCD` | 4 |
| 4 | version (`1`) | 2 |
| 6 | section count | 2 |
| 8 | total length (whole file) | 4 |
| 12 | reserved (`0`) | 4 |
| 16 | section table: `kind u8, pad[3], offset u32, len u32` × count | 12 × count |
| … | payloads, packed in table order | — |

Canonical-form rules (enforced on decode, so `decode(encode(c)) == c` and no
overlap/gap encodings are representable):

- table sorted strictly ascending by section kind (duplicates impossible),
- payload offsets exactly contiguous, starting right after the table,
- declared total length must equal the input length.

Sections (kind → name, bound, required):

| kind | name | bound | required |
| --- | --- | --- | --- |
| 0 | `header.json` | 16 KiB | yes |
| 1 | `frames.json` | 64 KiB | yes |
| 2 | `maps.json` | 64 KiB | yes |
| 3 | `logs.jsonl` | 256 KiB | no |
| 4 | `spans.jsonl` | 256 KiB | no |
| 5 | `regs.bin` | 4 KiB | no |

Whole-container bound: 1 MiB. `header.json` keys are stable
(`format, format_version, timestamp_nsec, pid, code, name, build_id`);
`frames.json` records carry `pc, build_id, function, file, line` with the
symbolization fields `null` until resolved.

The zstd wrapper (`.nxcd.zst`) sits **outside** the core format: it compresses
the already-encoded container bytes. Decompression is streamed against the
1 MiB container bound, so decompression bombs are rejected without buffering.
The `zst` cargo feature of the `nxcd` crate is host-tool-only and must never be
enabled from an OS-graph crate (RFC-0009).

Implementation: `userspace/crash/nxcd/` (crate `nxcd`).

## `nxsym` — Build-ID keyed symbol index

- `nxsym index <elf>... -o symbols.nxsym`
  - Build-ID source order:
    1. `.note.gnu.build-id` (hex-encoded, lowercase);
    2. fallback: `crash::deterministic_build_id(<file stem>)` — the exact
       function the OS producer stamps into `MinidumpFrame.build_id` for
       payloads without an embedded id, so index keys and dump keys never drift.
  - Per binary, every text symbol is resolved once through DWARF
    (`addr2line`) and stored as `addr → (function, file, line)` ranges.
  - Index file is CBOR with stable ordering (binaries by Build-ID, entries by
    address); reads are size-bounded and invariant-validated (fail-closed).
- `nxsym addr2line --sym symbols.nxsym --addr 0x... [--build-id <id>]`
  - resolves against the index only (the ELF is not needed at lookup time);
    `--build-id` defaults to the sole binary for single-binary indexes.

Known limitation (documented on purpose): resolution granularity is the
function entry (`line` = line of the function definition), not the exact
statement for mid-function addresses. Full line-table indexing is v2b scope.

Implementation: `tools/nxsym/` (library + CLI).

## `nx crash` operator commands

See `docs/dev/nx-cli.md` for the full flag reference. Summary:

- `ls` — deterministic listing of a dump directory; corrupt dumps are flagged,
  not fatal.
- `show` — decode one dump; `--sym` symbolizes frames using the Build-ID
  carried in each frame record.
- `export` — convert any supported input (`.nmd`, `.nxcd`, `.nxcd.zst`) to the
  canonical `.nxcd.zst` (or plain `.nxcd`).
- `purge` — budgeted GC (`--max-bytes`, `--max-count`, `--dry-run`): keeps the
  newest dumps that fit both budgets; the plan is a pure function
  (`nxcd::plan_purge`) and fully deterministic, including timestamp ties.
  Invalid dump files are never deleted by budget logic.
- `grep` — substring search across header, frames, modules, and log/span text.

All dump inputs are treated as untrusted: reads are size-bounded before
parsing and every malformed input maps to a deterministic reject
(exit class `3`, `validation_reject`).

## On-device writer (v2b, TASK-0051B)

The at-rest artifact is the canonical **plain `.nxcd`** container (the zstd
wrapper stays host-tool-only per RFC-0009 dependency hygiene; `nx crash
export` produces `.nxcd.zst`). The writer is an execd-side library call —
there is no `crashd` daemon (authority registry decision).

- **Flow**: crash reap → bounded NMD1 frame → `nxcd::from_minidump_with_reason`
  (the ADR-0056 exit reason rides `header.json`) → preview sections per the
  redaction level → PUT `/state/crash/<ts>.<pid>.<name>.nxcd` → the `.nmd`
  intermediate is deleted. Conversion failure degrades LOUD
  (`crash: container write degraded (reason=…)` — fatal in proof boots) and
  keeps the raw NMD1 so evidence is never lost.
- **Redaction** (policyd, deny-by-default): `crash.attach.full` →
  `crash.attach.stack` → none; the conservative granted default is
  stack-only (`stack.bin` ≤ 4 KiB + `code.bin` ≤ 256 B — exactly the NMD1
  preview bounds; NMD1 carries no full-memory section). Resolved once per
  boot; policyd unreachable = attach nothing.
- **Placement**: statefs KV records under `/state/crash/` — per-artifact cap
  32 KiB (a legal frame converts to ~12 KiB), ADR-0043-shaped. Bulk-scale
  captures move to `/data` with TASK-0317.
- **Retention**: once per boot, before the first publish —
  `crash: retention gc on (budget=256KiB)`; `nxcd::plan_purge` over
  `/state/crash/` (max 8 artifacts / 256 KiB total, `.nmd` leftovers
  included), deletions marked (`crash: retention gc deleted (n=…)`) and
  audited as an evidence-class record (`execd.audit` scope, 0049C ring).
- **Markers**: `crash: dump written (id=… bytes=…)` (required),
  `SELFTEST: crash artifact ok` (container magic verified + intermediate
  gone), `SELFTEST: crash redaction ok` (allowed sections only, previews
  exactly per level).
- **Host round trip** (documented check): after a QEMU run,

  ```bash
  nx diagnose --image build/data.img --out bundle.tar   # bundles crash/<id>.nxcd verbatim
  tar -xf bundle.tar
  nx crash ls --dir crash/                              # finds the artifacts
  nx crash show crash/<id>.nxcd --sym symbols.nxsym     # symbolizes (0048 pipeline)
  ```

## Deferred

- VMO artifacts (filebuffer path only until a consumer proves the need).
- Log/trace correlation (`logs.jsonl` / `spans.jsonl` producers) — evidence
  records already survive at rest (0049C) and `nx diagnose` correlates
  host-side, so they are deliberately NOT copied into every container.
- Export/notification surface (TASK-0141) + Problem Reporter UI (TASK-0142).
- Packaging integration (embedding symbol indices into `.nxb`/`.nxs`) — gated
  on packaging-format stability; v2a deliberately does not touch the packers.
- Exact line-table symbolization.
