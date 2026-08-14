---

title: TASK-0034 Delta updates v1: nxdelta (rollsum+zstd) + bundlemgrd delta apply (digest/bootctl goals shipped)
status: Draft
owner: @runtime
created: 2025-12-22
updated: 2026-08-14
size: M  # was L; Goals 1+2 shipped (see Rebase 2026-08-14), residual scope is the .nxdelta lane only
depends-on: []
follow-up-tasks: []
links:

  - Vision: docs/architecture/vision.md
  - Packaging baseline: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Manifest format: docs/adr/0020-manifest-format-capnproto.md
  - Signing policy: docs/security/signing-and-policy.md
  - Supply-chain baseline (SBOM/sign policy): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Persistence substrate (resume checkpoints): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - VMO plumbing (optional fast path): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md


  - TASK-0007: Updates v1.0 (manifest.nxb unification, non-persistent A/B skeleton)
  - TASK-0009: Persistence v1 (statefs for bootctl + resume checkpoints)
---

## Rebase 2026-08-14 — what already shipped (do NOT re-implement)

Verified against the repo on 2026-08-14. Goals 1 and 2 of this ledger are
**delivered and QEMU-gated**; only the delta lane (Goal 3) remains.

**Goal 1 — v1.1 manifest digest fields: SHIPPED.**

- Schema: `tools/nexus-idl/schemas/manifest.capnp:13` carries the changelog
  entry "v1.1: Add payloadDigest + payloadSize (TASK-0034)"; the fields live at
  `manifest.capnp:67` (`payloadDigest`) and `manifest.capnp:71` (`payloadSize`).
- Producer: `tools/nxb-pack/src/main.rs:188` computes SHA-256(payload.elf).
- Verifier: `source/services/bundlemgrd/src/std_server.rs:747-751` verifies the
  digest on install; SBOM check at `:824`, repro check at `:839`, signature
  policy at `:1041`.

**Goal 2 — persistent bootctl: SHIPPED.**

- `updated` is a real 971-LOC service, not a skeleton:
  `source/services/updated/src/os_lite.rs` with `handle_stage` (:396),
  `handle_switch` (:463), `handle_health_ok` (:516), `handle_boot_attempt`
  (:555).
- Persistence: `os_lite.rs:53` `BOOTCTRL_STATE_KEY = "/state/boot/bootctl.v1"`.
- Marker: shipped as `updated: ready (statefs)`, gated at
  `scripts/qemu-test.sh:482`. (This ledger previously said
  `updated: ready (persistent)` — the shipped string is the contract.)
- The full OTA ladder is QEMU-gated at `scripts/qemu-test.sh:536-540`
  (`SELFTEST: ota stage/switch/health/rollback ok`), with negative proofs in
  `proof-manifest markers/ota.toml:39-54`.
- Host proof exists: `tests/updates_host/tests/ota_flow.rs` (11 tests, incl.
  `rollback_on_health_timeout` :105 and `reject_mismatched_digest` :92) over
  `userspace/updates/src/bootctrl.rs` (stage :82, switch/tries_left :88,
  commit_health :102, tick_boot_attempt :113 auto-rollback, rollback :127).

**Honest residual scope (all that is left of this task):**

- The `.nxdelta` on-disk format, `tools/nxdelta/` (make/apply CLI + library),
  resume/checkpoint, and the `bundlemgrd` delta-apply path. Nothing named
  `nxdelta` exists anywhere in the repo today (`tools/` has `nxb-pack`,
  `nxs-pack`, `pkgr`, `pkgimg-build`).
- **RFC seed required before building**: `.nxdelta` is a new on-disk wire
  format, so per the CLAUDE.md workflow rule it needs an RFC seed
  (`docs/rfcs/RFC-TEMPLATE.md`, next free number, update the RFC index) before
  implementation starts.

Corrections to stale flags below (kept struck-through for history):

- The old RED flag "`.nxs`/`updated` do not exist yet" is dead: `.nxs` is live
  end-to-end (`source/apps/selftest-client/build.rs:53` generates
  `system-test.nxs`; parser `userspace/updates/src/system_set.rs`, 441 LOC).
- The old YELLOW "tooling still writes manifest.json" is dead:
  `manifest.capnp` is the shipped contract (see Goal 1 evidence above).

## Context

We want bandwidth-efficient bundle updates via binary deltas:

- produce and apply delta patches deterministically,
- support resume/checkpoint after interruption,
- verify integrity + signature policy **before** committing an installed bundle.

**This task also includes v1.1 features moved from TASK-0007**:

- **Per-bundle digest/size fields** in `manifest.nxb` (schema v1.1)
- **Persistent bootctl** integration (after TASK-0009)
- **Digest verification** on bundle install

Repo reality (superseded by the Rebase 2026-08-14 section above — kept for
history):

- `updated` service exists (now persistent, 971 LOC — see Rebase section)
- `manifest.nxb` (Cap'n Proto) is unified repo-wide
- `.nxs` tooling exists for system-set packaging
- Bundle install/verify exists via `bundlemgrd`
- Persistence substrate (TASK-0009) provides `/state` for bootctl + checkpoints

This task is **bundle-only**, **host-first**, and **OS-gated**.

## Goal

Deliver:

1. **v1.1 manifest fields** (from TASK-0007) — ✅ **DELIVERED** (see Rebase
   2026-08-14; do NOT re-implement):
   - `payloadDigest` + `payloadSize` in `manifest.capnp` (:67/:71)
   - `nxb-pack` computes SHA-256(payload.elf) (`tools/nxb-pack/src/main.rs:188`)
   - `bundlemgrd` verifies digest on install (`std_server.rs:747-751`)

2. **Persistent bootctl** (from TASK-0007) — ✅ **DELIVERED** (see Rebase
   2026-08-14; do NOT re-implement):
   - `updated` integrated with statefs (`os_lite.rs:53`,
     `BOOTCTRL_STATE_KEY = "/state/boot/bootctl.v1"`)
   - Marker: `updated: ready (statefs)` (shipped string; gated at
     `scripts/qemu-test.sh:482`)

3. **Delta format and tooling** (`.nxdelta`) — **RESIDUAL SCOPE** (RFC seed
   for the on-disk format required first):
   - Deterministic delta format (rollsum + zstd)
   - Bundle-level apply flow
   - Resume/checkpoint support
   - Verify integrity before commit

## Non-Goals

- System-set (`.nxs`) delta container and orchestration (separate task).
- Kernel changes.
- Claiming “zero-copy” unless VMO sharing is proven end-to-end.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic format and output (stable ordering, stable chunk sizes).
- Bounded memory:
  - capped rolling-window index
  - capped in-flight output buffers
  - bounded record sizes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers (OS markers only after real apply+verify+commit behavior exists).

## Red flags / decision points

- ~~**RED (system delta gating)**: `.nxs`/`updated` do not exist yet.~~
  **RESOLVED 2026-08-14**: both exist and are QEMU-gated (see Rebase section).
  System-set delta orchestration still stays out of this task (TASK-0035).
- ~~**YELLOW (manifest drift)**: tooling still writes `manifest.json` in some
  paths.~~ **RESOLVED 2026-08-14**: `manifest.capnp` v1.1 is the shipped
  contract; delta logic operates on payload bytes + canonical digests.
- **RED (new wire format)**: `.nxdelta` needs an RFC seed before
  implementation (CLAUDE.md workflow rule for new on-disk formats).
- **YELLOW (VMO fast path feasibility)**:
  - VMO-based apply can be added as an optional optimization only after VMO sharing/transfer is proven (TASK-0031).

## Contract sources (single source of truth)

- Supply-chain policy: TASK-0029
- Persistence: TASK-0009
- QEMU marker contract: `scripts/qemu-test.sh` (gated)

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/nxdelta_host/`):

- make/apply: base+target → patch → applied output is byte-identical to target
- corruption: tamper ADD block → apply fails deterministically (integrity error)
- resume: interrupt apply mid-stream, persist checkpoint, restart apply → completes and verifies
- determinism: running `make` twice produces identical patch bytes for identical inputs.

### Proof (OS / QEMU) — gated

Once bundle install/update paths exist in OS builds with statefs:

- `bundlemgrd: delta apply start (bundle=<...>)`
- `bundlemgrd: delta verify ok`
- `bundlemgrd: delta commit ok`
- `SELFTEST: delta bundle apply ok`
- `SELFTEST: delta bundle resume ok`
- `SELFTEST: delta integrity deny ok`

Notes:

- Any postflight must delegate to canonical harness/tests; no independent “log greps = success”.

## Touched paths (allowlist)

- `docs/rfcs/` (new: RFC seed for the `.nxdelta` on-disk format — first PR)
- `tools/nxdelta/` (new: format + make/apply)
- `tests/` (new: host tests)
- `source/services/bundlemgrd/` (apply+verify+commit; OS-gated)
- `source/apps/selftest-client/` (OS-gated markers)
- `docs/updates/delta.md`
- `docs/testing/README.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Define `.nxdelta` format + library**
   - LE header includes:
     - kind=NXB (bundle)
     - base sha256, target sha256
     - chunk size, algorithm string
   - Records:
     - `COPY { off, len }`
     - `ADD { zstd-compressed bytes }`
   - Trailer includes `records_sha256` for integrity of the patch stream itself.

2. **Host CLI**
   - `nxdelta make --base --target -o patch.nxdelta`
   - `nxdelta apply --base --patch -o out`
   - Deterministic emission: stable scanning order, stable zstd parameters.

3. **Resume / checkpoint**
   - Define a checkpoint file format (JSON/CBOR) containing:
     - patch digest, base digest, target digest
     - last record index applied
     - output digest-so-far (or rolling verification state)
   - Host tests prove resume semantics.
   - OS: checkpoint stored under `/state/update/delta/<bundle>.ckpt` (gated on statefs).

4. **bundlemgrd integration (OS-gated)**
   - Apply patch to a staging area (file or VMO) using streaming reads.
   - Verify:
     - target sha256 matches
     - manifest/SBOM digest checks per TASK-0029
     - signature policy (publisher/key allowlist) per TASK-0029
   - Commit atomically (swap staged bundle and update bundle index).

5. **Docs**
   - `docs/updates/delta.md` describing `.nxdelta`, resume, and verification-before-commit.

## Follow-ups (separate tasks)

- System-set delta container + updated orchestration (see TASK-0035).
- VMO fast path for apply once VMO sharing/transfer is proven.
