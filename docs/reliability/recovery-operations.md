<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Recovery operations surface (TASK-0051)

Status: shipped — fsck op + bootctld ops QEMU-proven (reset lane), `nx diagnose` host-tested.

## CONTEXT

- Scope: `TASK-0051` — statefsd fsck ops, bootctld slot/target ops with the
  recovery commit block, `nx diagnose` (the ONE diagnostic bundle, Keystone
  Gate 6).
- Contracts: RFC-0087 §4 (boot targets/one-shot consumption),
  ADR-0055 (bootctld = single boot-state authority),
  `userspace/statefs/src/fsck.rs` (ONE repair semantics; host twin
  `tools/fsck-statefs`).
- Proof commands: `cargo test -p statefsd --test fsck_op_contract`,
  `cargo test -p statefs` (engine matrix + streaming-window cases),
  `cargo test -p nx --test diagnose_cli`,
  `scripts/qemu-test.sh --profile=reset` (three-boot recovery cycle).

## statefsd fsck ops

Wire ops `OP_FSCK_CHECK` (11) / `OP_FSCK_REPAIR` (12) on the statefs protocol
run the delivered 0026/0027 engine against the mounted store:

- **Policy**: CHECK needs `statefs.read`, REPAIR needs `statefs.admin`
  (deny-by-default via policyd; denials audit as access-denied).
- **Quiesce**: both ops reject with `STATUS_BUSY` (11) while any transaction
  is open — fsck re-opens the journal from the raw device and would silently
  drop RAM-staged transactions. Marker: `statefsd: fsck busy (open txns)`.
- **Engine swap**: the live engine is replaced by a mem placeholder, fsck
  consumes the device, then the store re-opens (fresh `SeqTracker`, enrolled
  replay, enc re-enable). A store that cannot re-open degrades LOUD:
  `statefsd: fsck fail (unrecoverable)`.
- **Repair bound**: structural — repair appends at most one ABORT per orphan
  and open transactions are capped at `MAX_OPEN_TXNS` (8).
- **Report**: fixed 34-byte wire payload (see `statefsd::fsck_op`), status
  mirrors the fsck-statefs exit-code contract (clean-with-enc-failures is
  NOT ok).
- **Memory truth**: the scan streams the journal region through a bounded
  window (`statefs::fsck_window`), so the op survives statefsd's 1 MiB heap
  and a clean journal reads ~journal-length bytes, not the whole region.

Markers: `statefsd: fsck check ok (clean)` · `statefsd: fsck repaired (n=<k>)`
· `statefsd: fsck fail (unrecoverable)`.

## bootctld slot/target ops

Schedule-only mutations at the single boot-state authority (`'B','T'` wire v1):

- `OP_GET_TARGET` (6), `OP_SET_NEXT_BOOT` (7), `OP_SET_TARGET` (8) — target
  reads/writes need the delegated `boot.target` capability.
- `OP_RESET` (9) — SBI SRST via the identity-bound kernel syscall; needs
  `boot.reset`.
- `OP_GET_RECORD` (11) — read-only record snapshot for diagnostics.
- **Commit block**: while the session graph is `recovery`, every slot
  mutation (STAGE/SWITCH/HEALTH_OK/ROLLBACK) rejects with
  `STATUS_COMMIT_BLOCKED` (5) BEFORE the sender gate — provable from the
  recovery boot itself. Marker: `bootctld: commit blocked (target=recovery)`.
- **Persist-or-restore**: the on-disk record and the RAM machine commit
  together or not at all; `bootctld: switch scheduled (to=<slot>)` is emitted
  only after the record persisted (required in the OTA ladder).

## `nx diagnose`

The ONE diagnostic bundle (host-side assembly; TASK-0227 owns the format
lineage and rebases onto these sections):

```bash
# after any QEMU run that left a store image:
nx diagnose --image build/data.img --out nx-diagnose.tar --json
```

> Image naming caveat: virtio-mmio slots enumerate in reverse device
> order, so the statefs journal currently lands in `build/data.img` (the
> launcher's `blk.img`/`data.img` name↔role comment is swapped; keep-blk
> lanes preserve both, which is why nothing ever noticed). Verify with
> `fsck-statefs <image>` if in doubt — the statefs image replays.

- Sections (fixed order): `diagnose/boot-record.json` (bootctld record via
  `open_record`), `diagnose/evidence.jsonl` (logd evidence ring, oldest
  first), `diagnose/fsck-report.json`, `diagnose/meta.json`.
- Deterministic by construction: same image bytes ⇒ byte-identical archive
  (hand-rolled ustar, mtime 0, no host timestamps). Plain `.tar` — a
  compression layer would trade determinism for nothing at these sizes.
- The fsck OUTCOME is data inside the bundle; `nx` exit classes stay the CLI
  contract (missing image = `missing_dependency`, unaligned image =
  `validation_reject`).

## Recovery-cycle proof (reset lane)

`scripts/qemu-test.sh --profile=reset` drives THREE boots in one UART stream:
normal (arm `next_boot=recovery`) → recovery graph (quiesce-busy → fsck check
clean → commit block → `SELFTEST: recovery slot ok`) → normal (one-shot
consumed, ungranted mutating op denies: `SELFTEST: recovery ops deny ok`).
