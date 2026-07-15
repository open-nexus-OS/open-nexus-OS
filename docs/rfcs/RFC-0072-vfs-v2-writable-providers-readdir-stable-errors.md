# RFC-0072: VFS v2 — ReadDir, writable providers, and the stable storage error SSOT — contract seed

- Status: Draft (2026-07-15)
- Owners: @runtime
- Created: 2026-07-15
- Last Updated: 2026-07-15
- Links:
  - Tasks: `tasks/TASK-0291-vfs-readdir-svc-files-stash-real-listing.md` (P1 execution + proof), `tasks/TASK-0293-nxfsd-os-bringup-gpt-mount-data-keepblk.md` (P2 execution + proof), `tasks/TASK-0295-zero-copy-read-write-vmo-splice.md` (P3)
  - ADRs: `docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md`
  - Related RFCs: `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` (namespace/CapFd discipline this RFC composes with), `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md` (RO provider), `docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md` (the RW provider), `docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md` (app surface on top), `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` (data plane)
  - Track: `tasks/TRACK-STASH-USER-DATA-FS.md`
  - Absorbs: `tasks/TASK-0132-storage-errors-vfs-semantic-contract.md` (the stable error-semantics contract lives here now; that task's execution folds into TASK-0291).

## Status at a Glance

- **Phase 1 (ReadDir + stable error codes on the read-only surface)**: ⬜ — `TASK-0291`
- **Phase 2 (write ops + writable provider registration, `/data` via nxfsd)**: ⬜ — `TASK-0293`
- **Phase 3 (VMO handle data plane for large reads/writes)**: ⬜ — `TASK-0295`

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The `vfs.capnp` v2 surface: `ReadDir` (bounded pagination), write ops (`Create`, `Write`,
    `Truncate`, `Mkdir`, `Rename`, `Remove`), and the versioning rule for extending it.
  - The **stable storage error SSOT**: one numeric error-code table shared by vfsd and all providers,
    and the mapping discipline for provider-internal errors.
  - The provider contract: how a filesystem (packagefsd, nxfsd) registers a mount as RO or RW and
    which ops it must implement vs. may reject.
  - The interaction rule with sandboxing (RFC-0042): namespace filtering and CapFd checks happen
    **before** provider dispatch, for the new ops exactly as for the old ones.
- **This RFC does NOT own**:
  - Provider internals (on-disk formats, journals) — RFC-0018/0041/0071.
  - The app-facing DSL surface, permissions, roles — RFC-0073.
  - Kernel VMO semantics — RFC-0040.

### Relationship to tasks (single execution truth)

- `TASK-0291` implements + proves Phase 1; `TASK-0293` Phase 2; `TASK-0295` Phase 3.
- If a task needs surface not defined here, that is a new contract seed — this RFC stays closed.

## Context

vfsd today (RFC-0042 era, `source/services/vfsd/`) exposes exactly `Open/Read/Close/Stat/Mount`
(`tools/nexus-idl/schemas/vfs.capnp`), is read-only by design, and reports failures as a bare
`ok: false` — there is no way to distinguish "not found" from "access denied" from "I/O error".
There is **no directory enumeration at all**: a file manager cannot even list `/packages`.
With nxfs (RFC-0071) arriving as the first writable provider and stash becoming a real file manager
(RFC-0073), the VFS surface needs directory listing, write ops, and — before more consumers bake
`ok:false` handling in — a stable error contract. The old `TASK-0132` asked for exactly that error
contract; it folds in here.

## Goals

- `ReadDir` with deterministic order and bounded pagination (no unbounded replies over 8 KiB frames).
- Stable numeric error codes on **every** response (additive capnp change), one table for all
  storage services, documented in one place.
- Write ops sufficient for a real file manager: create, write, truncate, mkdir, rename, remove.
- Provider registration declares RO/RW; RO providers reject writes with `EROFS` deterministically.
- Sandboxing composition: new ops are namespace-filtered and CapFd-gated identically to Open/Stat.

## Non-Goals

- Streaming/watch APIs (change notification) — future seed.
- POSIX byte-range locking, xattrs, symlink surface — not promised.
- The zero-copy fast path itself (Phase 3 wires VMO handles; the kernel guarantees are RFC-0040/TASK-0290).
- Client library ergonomics beyond `nexus-vfs` parity with the new ops.

## Constraints / invariants (hard requirements)

- **Determinism**: ReadDir order is the provider's canonical order (nxfs: byte-order of names;
  packagefs: index order) and is stable across identical mounts; pagination cursors are opaque but
  deterministic.
- **Bounded resources**: `limit` ≤ 64 entries per ReadDir reply; entry name ≤ 255 bytes; reply fits
  the 8 KiB frame; path length ≤ 1024; bounded open-handle table per client (cap: 64).
- **No fake success**: `ok=true` only after the provider actually performed the op (writes: after
  provider-side commit per its durability contract).
- **Additive schema evolution**: new capnp fields are appended, never renumbered; old clients keep
  working (they just don't read `err`).
- **Fail-closed**: unknown op / malformed request → error, never ignored; RO mount + write op →
  `EROFS`, never silent drop.
- No `unwrap`/`expect` in vfsd; bounded parsing on every request.

## Proposed design

### Contract / interface (normative)

**Stable storage error SSOT** (`ErrorCode`, shared table; canonical documentation
`docs/storage/errors.md` generated/maintained alongside the schema):

| code | name | meaning |
|---|---|---|
| 0 | `OK` | success |
| 1 | `ENOTFOUND` | path/object does not exist |
| 2 | `EACCES` | denied by policy/namespace/CapFd |
| 3 | `EROFS` | write op on read-only mount |
| 4 | `ENOTDIR` | path component is not a directory |
| 5 | `EISDIR` | file op on a directory |
| 6 | `EEXIST` | create-exclusive target exists |
| 7 | `ENOSPC` | provider out of space |
| 8 | `E2BIG` | size/limit cap exceeded |
| 9 | `EINTEGRITY` | checksum/AEAD validation failed (fail-closed) |
| 10 | `EBUSY` | object in use (e.g. open handles on remove) |
| 11 | `EINVAL` | malformed request (bad name, bad cursor, bad handle) |
| 12 | `EUNSUPPORTED` | op not supported by this provider/phase |
| 13 | `EIO` | underlying device error |

Rules: providers map internal errors into this table (statefs keeps its wire statuses but the
vfsd-visible surface uses this table); codes are append-only; every code ≥1 has at least one
`test_reject_*` negative test somewhere in the tree.

**Schema additions** (`tools/nexus-idl/schemas/vfs.capnp`, additive):

```capnp
struct DirEntry { name @0 :Text; kind @1 :UInt16; size @2 :UInt64; }

struct ReadDirRequest  { path @0 :Text; cursor @1 :UInt32; limit @2 :UInt16; }
struct ReadDirResponse { ok @0 :Bool; entries @1 :List(DirEntry);
                         nextCursor @2 :UInt32; eof @3 :Bool; err @4 :UInt16; }

# err field appended to every existing response (OpenResponse.err @4, ReadResponse.err @2,
# CloseResponse.err @1, StatResponse.err @3, MountResponse.err @1); 0 = OK.

struct CreateRequest   { path @0 :Text; kind @1 :UInt16; exclusive @2 :Bool; }   # Phase 2
struct WriteRequest    { fh @0 :UInt32; off @1 :UInt64; bytes @2 :Data; }        # Phase 2 (inline ≤ 4 KiB)
struct TruncateRequest { fh @0 :UInt32; size @1 :UInt64; }                       # Phase 2
struct MkdirRequest    { path @0 :Text; }                                        # Phase 2
struct RenameRequest   { from @0 :Text; to @1 :Text; }                           # Phase 2 (atomic within one mount)
struct RemoveRequest   { path @0 :Text; }                                        # Phase 2
# Phase 3 adds VMO-handle variants for bulk read/write per RFC-0040; inline Data above
# INLINE_IO_MAX = 4096 bytes is E2BIG from Phase 3 on (announced now, enforced then).
```

`kind` values (shared with StatResponse): `0 = file`, `1 = dir` (others reserved).

**ReadDir semantics**: `cursor = 0` starts; server returns ≤ `min(limit, 64)` entries in canonical
order plus `nextCursor`/`eof`; a cursor from a previous listing generation MAY return `EINVAL`
(clients restart) — no snapshot isolation promised in v2. Entries never include `.`/`..`.

**Provider contract**: a mount registers `{ fsId, writable: bool }`. vfsd rejects write ops on
non-writable mounts (`EROFS`) **before** provider dispatch. Writable providers MUST implement all
Phase 2 ops or answer `EUNSUPPORTED` per-op deterministically. `Rename` across mounts → `EUNSUPPORTED`.

**Sandbox composition (normative)**: RFC-0042 `NamespaceView` path filtering and CapFd validation
run identically for ReadDir and all write ops; a namespace that hides a subtree hides it from
ReadDir output too (entries filtered, not error). Deny → `EACCES` + audit (same sink as today).

### Phases / milestones (contract-level)

- **Phase 1** (`TASK-0291`): `err` fields + `ReadDir` on the RO surface (packagefs provider),
  `nexus-vfs` client API, error-code negative tests.
- **Phase 2** (`TASK-0293`): write ops + writable registration, first RW provider = nxfsd `/data`.
- **Phase 3** (`TASK-0295`): VMO-handle bulk IO variants; inline cap enforcement.

## Security considerations

- **Threat model**: confused-deputy listing/writing outside a sandbox (→ namespace filtering before
  dispatch); cursor forgery (cursors are validated indices, `EINVAL` on garbage); oversize/name
  attacks (bounded caps, `E2BIG`/`EINVAL`); RO bypass (`EROFS` enforced in vfsd, not delegated).
- **Mitigations**: deny-by-default routes (policyd), RFC-0042 canonicalization + CapFd HMAC/replay
  guard reused untouched, bounded replies.
- **Open risks**: no rate limiting per subject in v2 (bounded handles/reply sizes only); listing
  metadata (names/sizes) is visible to any subject that holds the route + namespace — per-app
  namespaces (RFC-0073 policy) are the real confinement line.

## Failure model (normative)

- Every response carries `err` (0 = OK); `ok` stays for wire compat and MUST equal `err == 0`.
- Partial ReadDir on provider error mid-iteration: return the error, no partial entries.
- Write durability: `ok` for writes means provider-accepted per its contract (nxfs: journaled txn);
  `Sync`-class guarantees are the provider's contract (RFC-0071), not invented here.
- No silent fallback anywhere: unsupported op → `EUNSUPPORTED`, never emulated approximately.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p vfsd
cd /home/jenning/open-nexus-OS && cargo test -p nexus-vfs
```

Covers: ReadDir pagination determinism (exact page boundaries), every error code's negative test
(`test_reject_*`), namespace-filtered listing, EROFS on RO mounts, cursor-garbage rejection.

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (if applicable)

- `vfsd: readdir ok (mount=/packages entries=<n>)` (Phase 1)
- `SELFTEST: vfs readdir ok`
- `SELFTEST: vfs write denied on ro ok` (Phase 1 negative)
- `vfsd: rw mount ok (/data)` (Phase 2)

## Alternatives considered

- **Per-provider ad-hoc listing ops** (packagefs `FileEntry` reuse) — rejected: two listing
  surfaces, no shared error semantics; the whole point is one VFS surface.
- **Unbounded ReadDir replies** — rejected: 8 KiB IPC frames make truncation-by-accident inevitable;
  bounded pagination is the honest contract.
- **Error strings instead of codes** — rejected: not stable, not testable, bloats frames.
- **Snapshot-isolated directory cursors** — deferred: needs provider snapshot support (nxfs Phase 3);
  contract explicitly allows `EINVAL`-restart until then.

## Open questions

- Should `ReadDir` optionally return per-entry mtime for file-manager sorting in Phase 2, or is
  Stat-per-entry acceptable until a batched-stat op is seeded? (owner: @runtime, decide in TASK-0293
  while wiring stash's detail pane.)

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 1**: ReadDir + err SSOT on RO surface — proof: `cargo test -p vfsd` + marker `vfsd: readdir ok (mount=/packages entries=<n>)` (TASK-0291)
- [ ] **Phase 2**: write ops + RW provider registration (`/data` via nxfsd) — proof: `vfsd: rw mount ok (/data)` (TASK-0293)
- [ ] **Phase 3**: VMO bulk IO + inline cap enforcement — proof: TASK-0295 gates
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*` per error code).
