<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Crashdump v2 (host-first pipeline)

Status: v2a shipped (host); OS-side ingestion (`crashd`) is deferred to `TASK-0049`.

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

## Deferred (v2b / TASK-0049)

- OS-side ingestion (`crashd`), VMO artifacts, retention policy, redaction.
- Log/trace correlation (`logs.jsonl` / `spans.jsonl` producers).
- Packaging integration (embedding symbol indices into `.nxb`/`.nxs`) — gated
  on packaging-format stability; v2a deliberately does not touch the packers.
- Exact line-table symbolization.
