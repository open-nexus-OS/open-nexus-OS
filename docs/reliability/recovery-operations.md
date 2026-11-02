---
schema: 1
title: "Recovery operations"
role: reference
subsystem: storage
sources: [source/services/statefsd/src/fsck_op.rs, source/services/statefsd/src/fsck_os.rs, userspace/statefs/src/fsck.rs, source/services/bootctld/src, tools/nx/src/commands/diagnose.rs, tools/fsck-statefs]
contracts: [RFC-0087, ADR-0055]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Recovery operations

> **Purpose** — the recovery ops surface: statefsd fsck ops, bootctld target/reset ops with the recovery commit block, and the `nx diagnose` bundle.
> **Owns** — `source/services/statefsd/src/fsck_op.rs`, `source/services/statefsd/src/fsck_os.rs`, `tools/nx/src/commands/diagnose.rs`
> **Contracts** — [RFC-0087](../rfcs/RFC-0087-reliability-failure-model-v1.md) §4 (boot targets, one-shot `next_boot`), [ADR-0055](../adr/0055-bootctld-single-boot-state-authority.md) (bootctld is the single boot-state authority)
> **Invariants** — one repair semantics: `userspace/statefs/src/fsck.rs` (host twin `tools/fsck-statefs`). fsck never runs while a transaction is open. In the recovery graph every slot mutation is refused before the sender gate.
> **Read next** — [nxra.md](nxra.md), [crashdump-v2.md](crashdump-v2.md)

## statefsd fsck ops

| op | code | needs |
|---|---|---|
| `OP_FSCK_CHECK` | 11 | `statefs.read` |
| `OP_FSCK_REPAIR` | 12 | `statefs.admin` |

Policy is deny-by-default via policyd; denials audit as access-denied.

| aspect | behaviour |
|---|---|
| quiesce | both ops reply `STATUS_BUSY` (11) while any transaction is open — fsck re-opens the journal from the raw device and would drop RAM-staged transactions |
| engine swap | the live engine is swapped for a placeholder, fsck consumes the device, then the store re-opens (fresh `SeqTracker`, replay, encryption re-enabled) |
| repair bound | at most one ABORT appended per orphan; open transactions are capped at `MAX_OPEN_TXNS` (8) |
| report | fixed 34-byte payload (`FSCK_REPORT_WIRE_LEN`, layout in `fsck_op.rs`); status mirrors the `fsck-statefs` exit codes (clean with encryption failures is not ok) |
| memory | the scan streams the journal region through a bounded window (`userspace/statefs/src/fsck_window.rs`) |

Markers: `statefsd: fsck busy (open txns)` · `statefsd: fsck check ok (clean)` · `statefsd: fsck repaired (n=<k>)` · `statefsd: fsck fail (unrecoverable)` (store could not re-open).

## bootctld ops

Wire `'B','T'` v1 (`source/services/bootctld/src/wire.rs`).

| op | code | needs |
|---|---|---|
| `OP_STAGE` / `OP_SWITCH` | 1 / 2 | sender = `updated` (switch also accepts an `.nxra` token) |
| `OP_HEALTH_OK` | 3 | sender in the declared health quorum |
| `OP_GET_STATUS` | 4 | — |
| `OP_GET_TARGET` | 6 | — (read) |
| `OP_SET_NEXT_BOOT` | 7 | `boot.target` |
| `OP_SET_TARGET` | 8 | `boot.target` |
| `OP_RESET` | 9 | `boot.reset` and sender = init or selftest (SBI SRST via the identity-bound kernel syscall) |
| `OP_ROLLBACK` | 10 | sender = `updated` |
| `OP_GET_RECORD` | 11 | — (record snapshot for diagnostics) |
| `OP_GET_MEASURED` | 12 | — |

For switch, target and reset ops a sender without the standing grant can present a `.nxra` token instead ([nxra.md](nxra.md)).

- **Commit block**: while the session graph is `recovery`, STAGE/SWITCH/HEALTH_OK/ROLLBACK reply `STATUS_COMMIT_BLOCKED` (5) before the sender gate. Marker: `bootctld: commit blocked (target=recovery)`.
- **Persist-or-restore**: the on-disk record and the RAM machine commit together or not at all. `bootctld: switch scheduled (to=<slot>)` is emitted only after the record persisted.

## `nx diagnose`

```bash
nx diagnose --image <statefs.img> --out nx-diagnose.tar --json
```

- `--image` is a raw statefs journal image (default `build/blk.img`); a whole GPT disk is not decoded. The QEMU disk is one GPT image (`build/nexus.img`) whose `state` partition holds the journal.
- Archive sections, fixed order:

| section | source |
|---|---|
| `diagnose/boot-record.json` | bootctld record (`bootctld::record::open_record`) |
| `diagnose/evidence.jsonl` | logd evidence ring, oldest first (`logd::spill::decode_slot_value`) |
| `diagnose/fsck-report.json` | `statefs::fsck` verdict |
| `diagnose/meta.json` | bundle metadata |
| `crash/<id>.nxcd` | artifacts under `/state/crash/`, verbatim |

- Deterministic: same image bytes ⇒ byte-identical archive (hand-rolled ustar, mtime 0, no host timestamps, no compression).
- The fsck outcome is data in the bundle. Exit classes: missing image → `missing_dependency`; image not a whole number of blocks → `validation_reject`.

## Recovery-cycle proof (`reset` profile)

`just test-os reset` drives three boots in one UART stream:

1. normal boot arms `next_boot=recovery`;
2. recovery graph: fsck busy → fsck check clean → commit block → `SELFTEST: recovery slot ok`;
3. normal boot: one-shot consumed; an ungranted mutating op is denied → `SELFTEST: recovery ops deny ok`.

## Tests

`cargo test -p statefsd --test fsck_op_contract`, `cargo test -p statefs`, `cargo test -p nx --test diagnose_cli`.
