# ADR-0043: User data lives in a dedicated filesystem service (`nxfs`); statefs stays the service-state KV and is hardened, never extended into a file store

- Status: Accepted. Fixes the storage split before any user-data code exists, so statefs is never
  bent into a file store under delivery pressure.
- Created: 2026-07-15
- Builds on: ADR-0023 (statefs persistence architecture), RFC-0018 (statefs journal format v1, Complete),
  RFC-0041 (packagefs v2 RO image), RFC-0042 (sandboxing/VFS namespaces).
- Contract: `docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md`
- Execution (SSOT): `tasks/TRACK-STASH-USER-DATA-FS.md` (tasks `TASK-0292`, `TASK-0293`)

## Context

statefs (TASK-0009, Done) is a journaled KV store purpose-built for boot-critical **service state**:
255-byte keys under `/state/`, ~8 KiB effective values over IPC, full replay into an in-RAM
`BTreeMap`, whole-journal recovery, capability-gated per-prefix access. Its consumers are the boot
chain itself — keystored's device key, updated's bootctl, settingsd's prefs.

The platform now needs **user data**: real files and directories for the stash file manager, media,
documents — gigabyte-scale, streamed, eventually snapshot-able and encrypted per class. Two forces
tempt a shortcut: statefs already exists and already persists; and the write-hardening tasks
(TASK-0025/0026/0027) are open anyway. Meanwhile the old TASK-0182/0183 sketched a third thing — a
"securefsd" encrypted overlay on top of `/state`.

## Decision

1. **User data gets a dedicated filesystem service** (`nxfsd`, engine crate `userspace/nxfs`),
   designed per RFC-0071 (container/volumes, transactions, CoW track, integrity, encryption
   classes), mounted read-write at `/data` through vfsd.
2. **statefs is not extended.** Its contract (RFC-0018) stays Complete; its open tasks
   (TASK-0025/0026/0027) harden the existing KV semantics — authenticity, 2PC, compaction, record
   encryption for its own values — and nothing else. No file semantics, no large values, no
   directory model in statefs, ever.
3. **The securefsd overlay direction (TASK-0182/0183) is superseded.** Encrypted user data is an
   nxfs volume class (RFC-0071 Phase 4), not a separate overlay filesystem duplicating journal and
   atomicity logic above a KV store.
4. **One authority per store**: `/state` = statefsd (service KV), `/packages` = packagefsd (RO
   bundles), `/data` = nxfsd (user files). vfsd is the single client-facing surface over all three;
   no service wears two hats.

## Rationale

- statefs's data model is structurally wrong for files: KV keys are not paths, the 8 KiB IPC value
  ceiling forces chunking hacks, and full-RAM replay makes gigabyte payloads impossible. Fixing any
  of that means a new on-disk format — i.e. a new filesystem wearing statefs's name inside the
  boot-critical daemon.
- Blast-radius: statefs sits under keystored and updated. A user-data workload (fragmentation, GC,
  huge writes, app-driven churn) must never be able to degrade or corrupt the store the boot chain
  reads. Process and format isolation is the cheapest reliable wall.
- The split mirrors the platforms we benchmark against: a small trusted registry/preferences store
  distinct from the general-purpose CoW user filesystem.

## v1 process-boundary staging (amendment, 2026-07-15)

The core decision — user data lives in the **nxfs engine/format**, statefs stays the KV — is fully
honored. The secondary question of WHICH PROCESS runs the nxfs engine is staged:

- **v1**: `vfsd` hosts the nxfs `/data` provider **in-process** (the `nxfsd` crate's `DataStore`
  library). This avoids adding a boot-critical init service (endpoint, spawn, routes) in the same
  session that introduces the write path, so "stash writes + persists" could be delivered and
  boot-proven without destabilizing the boot chain.
- **Follow-up**: extract the `DataStore` into a standalone `nxfsd` process (full "one authority per
  store" separation) by wrapping it in a `KernelServer` loop and adding the vfsd→nxfsd route. The
  `DataStore` is written process-boundary-agnostic precisely so this extraction is mechanical.

This does not weaken the ADR's principle: the file store is a distinct engine with its own format
and device, never bent into statefs. The `/data` vs `/state` authority split is real at the
storage-engine and device level today; only the vfsd/nxfsd process split is deferred.

## Consequences

- **Positive**: statefs hardening (25–27) proceeds on a stable, small contract; nxfs can adopt an
  aggressive modern design (CoW, snapshots, encryption classes) without endangering boot; the
  stash/app surface gets one clean mount (`/data`) to grow into.
- **Cost**: two storage engines to maintain. Mitigated: both are host-first Rust crates sharing the
  `BlockDevice` trait, the CRC/journal discipline, and (post ADR-0044) the same partition layer —
  and the 2PC/fsck patterns TASK-0026 builds for statefs are the same patterns nxfs P1 needs.
- **Boundary rule for reviews**: any PR that adds file/path/large-value semantics to
  `userspace/statefs` or `statefsd` is rejected on sight and redirected to nxfs.
