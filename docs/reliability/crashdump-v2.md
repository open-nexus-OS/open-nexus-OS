---
schema: 1
title: "Crash dumps (.nxcd)"
role: spec
subsystem: observability
sources: [userspace/crash/nxcd, userspace/crash/src, tools/nxsym, source/services/execd/src/crash_os.rs, source/services/execd/src/crash_store.rs, tools/nx/src/commands/crash.rs]
contracts: [RFC-0031, RFC-0009, ADR-0056, ADR-0043]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Crash dumps (.nxcd)

> **Purpose** — the `.nxcd` container format, the Build-ID symbol index, the on-device writer in execd, and the `nx crash` host commands.
> **Owns** — `userspace/crash/nxcd/`, `tools/nxsym/`, `source/services/execd/src/crash_os.rs`, `source/services/execd/src/crash_store.rs`
> **Contracts** — [RFC-0031](../rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md) (minidump v1 + host symbolization), [RFC-0009](../rfcs/RFC-0009-no-std-dependency-hygiene-v1.md) (why zstd stays host-only), [ADR-0056](../adr/0056-task-exit-reason-kernel-abi.md) (the exit reason in `header.json`), [ADR-0043](../adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md) (`/state` is a KV of small records)
> **Invariants** — `build_id` comes from the producer and is carried verbatim; host tools never re-derive it for a dump. Every dump input is untrusted: size-bounded before parsing, malformed input is a deterministic reject. The `zst` feature of `nxcd` never enters the OS graph.
> **Read next** — [observability.md](observability.md), [recovery-operations.md](recovery-operations.md), [../dev/nx-cli.md](../dev/nx-cli.md)

## Pipeline

```
device (execd)                                  host
--------------                                  ----
crash reap → NMD1 frame (build_id embedded)
  → nxcd::from_minidump_with_reason
  → PUT /state/crash/<ts>.<pid>.<name>.nxcd  →  nx diagnose → crash/<id>.nxcd
                                                nx crash ls/show/export/purge/grep
                                                nxsym index / addr2line
```

There is no `crashd` daemon; the writer is a library call inside execd. There is one dump format: `.nxcd` at rest, `.nxcd.zst` for export.

## Container format v1

All integers little-endian.

| offset | field | size |
|---|---|---|
| 0 | magic `NXCD` | 4 |
| 4 | version (`1`) | 2 |
| 6 | section count | 2 |
| 8 | total length (whole file) | 4 |
| 12 | reserved (`0`) | 4 |
| 16 | section table: `kind u8, pad[3], offset u32, len u32` × count | 12 × count |
| … | payloads, packed in table order | — |

Canonical form, enforced on decode (so `decode(encode(c)) == c`):

- the table is sorted strictly ascending by kind (no duplicates);
- payload offsets are contiguous, starting right after the table;
- the declared total length equals the input length;
- an unknown kind rejects;
- the whole container is at most 1 MiB (`MAX_TOTAL_NXCD`).

### Sections

| kind | name | bound | required |
|---|---|---|---|
| 0 | `header.json` | 16 KiB | yes |
| 1 | `frames.json` | 64 KiB | yes |
| 2 | `maps.json` | 64 KiB | yes |
| 3 | `logs.jsonl` | 256 KiB | no |
| 4 | `spans.jsonl` | 256 KiB | no |
| 5 | `regs.bin` | 4 KiB | no |
| 6 | `stack.bin` | 4 KiB | no |
| 7 | `code.bin` | 256 B | no |

`stack.bin` and `code.bin` mirror the NMD1 preview bounds (`crash::MAX_STACK_PREVIEW` / `MAX_CODE_PREVIEW`), so the preview survives when the `.nmd` intermediate is deleted.

### JSON sections

| section | shape |
|---|---|
| `header.json` | keys in this order: `format` (`"nxcd"`), `format_version` (`1`), `timestamp_nsec`, `pid`, `code`, `name`, `build_id`, optional `reason` (kernel exit reason, omitted when absent) |
| `frames.json` | `{"frames": [{pc, build_id, function, file, line}]}`; the last three stay `null` until symbolized |
| `maps.json` | `{"modules": [{name, build_id}]}` |

### zstd wrapper

`.nxcd.zst` compresses the already-encoded container; it is outside the core format. Decompression streams against the 1 MiB bound, so a decompression bomb rejects without buffering. The `zst` cargo feature is host-tool-only.

## `nxsym` — Build-ID symbol index

| command | does |
|---|---|
| `nxsym index <elf>... -o symbols.nxsym` | indexes every text symbol once through DWARF into `addr → (function, file, line)` ranges |
| `nxsym addr2line --sym symbols.nxsym --addr 0x… [--build-id <id>]` | resolves from the index alone; `--build-id` is required for multi-binary indexes |

- Build-ID source: `.note.gnu.build-id` (lowercase hex); fallback `crash::deterministic_build_id(<file stem>)` — the same function execd stamps for payloads without an embedded id.
- The index is CBOR with stable ordering (binaries by Build-ID, entries by address); reads are size-bounded and fail closed.
- Limit: resolution is per function entry (`line` = line of the function definition), not per statement.

## `nx crash` (host)

| verb | does |
|---|---|
| `ls [--dir ./crash]` | deterministic listing; corrupt dumps are flagged, not fatal |
| `show <path> [--sym symbols.nxsym]` | decodes one dump; `--sym` symbolizes by each frame's `build_id` |
| `export <path> -o <out>` | converts `.nmd`, `.nxcd` or `.nxcd.zst` to `.nxcd.zst` or plain `.nxcd` |
| `purge [--dir] [--max-bytes N] [--max-count N] [--dry-run]` | keeps the newest dumps that fit both budgets (`nxcd::plan_purge`, deterministic incl. timestamp ties); invalid files are never deleted by budget logic |
| `grep <pattern> [--dir]` | substring search over header, frames, modules, log and span text |

Every verb takes `--json`. A malformed input exits with class `3` (`validation_reject`).

## On-device writer (execd)

| aspect | behaviour |
|---|---|
| flow | reap → bounded NMD1 frame → `.nxcd` with the exit reason in `header.json` → sections per redaction level → PUT `/state/crash/<ts>.<pid>.<name>.nxcd` → `.nmd` deleted |
| failure | conversion failure keeps the raw NMD1 and degrades loud: `crash: container write degraded (reason=…)` (fatal in proof boots) |
| redaction | policyd, deny-by-default, resolved once per boot: `crash.attach.full` → `crash.attach.stack` → none; policyd unreachable = attach nothing |
| artifact cap | 32 KiB per artifact (`ARTIFACT_MAX_BYTES`) |
| retention | once per boot before the first publish: `crash: retention gc on (budget=256KiB)`; `plan_purge` over `/state/crash/` with max 8 artifacts / 256 KiB (`GC_BUDGET`), `.nmd` leftovers included; deletions emit `crash: retention gc deleted (n=…)` and an `execd.audit` evidence record |

The at-rest artifact is plain `.nxcd`; only the host produces `.nxcd.zst`.

### Markers

| marker | means |
|---|---|
| `crash: dump written (id=… bytes=…)` | artifact persisted |
| `SELFTEST: crash artifact ok` | container magic verified, intermediate gone |
| `SELFTEST: crash redaction ok` | only allowed sections, previews exactly per level |

## Host round trip

```bash
nx diagnose --image <statefs.img> --out bundle.tar   # bundles crash/<id>.nxcd verbatim
tar -xf bundle.tar
nx crash ls --dir crash/
nx crash show crash/<id>.nxcd --sym symbols.nxsym
```

`nx diagnose` reads a raw statefs journal image (see [recovery-operations.md](recovery-operations.md)).

## Tests

`cargo test -p crashdump_v2_host`, `cargo test -p nxsym`, `cargo test -p nxcd --features zst`, `cargo test -p nx --test crash_cli` (`tools/nx/tests/crash_cli.rs`).

## Not in the format

- Log/span producers for `logs.jsonl` / `spans.jsonl`: evidence records already persist in the logd ring and `nx diagnose` correlates them host-side.
- Symbol indices embedded in `.nxb` / `.nxs`.
- Line-table (per-statement) symbolization.
