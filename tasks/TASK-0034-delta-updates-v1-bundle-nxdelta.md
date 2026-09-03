---

title: TASK-0034 Delta updates v1: nxdelta (rollsum+zstd) + bundlemgrd delta apply (digest/bootctl goals shipped)
status: Done
owner: @runtime
created: 2025-12-22
updated: 2026-09-01
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

## DELIVERED 2026-09-01 (RFC-0090; honest recuts stated)

Shipped in three packages (P1 format RFC + `userspace/nxdelta` · P2 engine
kind 3 + device adapter + `nx image ota --delta-from` + fixtures · P3
selftest lane + docs), test-all green. Ground-truth recuts vs. the plan
below — each an honesty or authority-rule fix:

1. **No `tools/nxdelta/` binary and no `nxdelta make/apply` CLI** —
   TRACK-AUTHORITY-NAMING forbids a second entrypoint; emission is
   `nx image ota --delta-from <base>`, the library is `userspace/nxdelta`
   (no_std streaming decoder + std deterministic emitter, RFC-0090).
2. **Kind number is 3** (RFC-0089 §3 reserves 2 = `bundle` for Phase B).
3. **No checkpoint files**: for a boot image, RFC-0089 §8 idempotent
   restage IS the resume story (host-proven: power-cut restage converges;
   torn stage leaves the slot NXBD-invalid). The checkpoint DoD below was
   written for BUNDLE deltas — those live behind the Phase-B seam with
   TASK-0035, where per-component checkpointing returns if the economics
   demand it.
4. **No zstd in v1** (`algo` 0 = stored ADDs; 1 reserved): admitting a
   decompressor into a no_std update-trust path is an RFC-0009 D4
   allowlist decision, not a format side effect. COPY coverage carries
   the bandwidth win (append-shaped 2 MB change ⇒ ~10 KB container).
5. **`bundlemgrd` delta apply**: moved behind Phase B with `bundle-delta`
   (per the 2026-08-25 rebase note) — the OS markers below
   (`bundlemgrd: delta *`, `SELFTEST: delta bundle *`) go with it. The
   shipped OS proof is the boot-image lane: `updated: stage rejected
   (delta-base)` -> `SELFTEST: ota delta base deny ok` -> `updated:
   component boot-image-delta verified` -> `SELFTEST: ota delta stage ok`
   (headless + smp1 gated; the reconstruction COPY-reads the ACTIVE slot).
6. Trust shape delivered stronger than planned: the manifest signature
   covers the DELTA STREAM (component sha256 = stream digest), so the
   decoder never parses unsigned bytes; the base binds O(1) against the
   loader-verified active NXBD instead of a full base hash.

## Rebase note 2026-08-25 (RFC-0089: the missing RFC seed exists; target recut)

The RED flag below ("`.nxdelta` requires an RFC seed before implementation") is
resolved: RFC-0089 §11 reserves the delta seam — deltas are `.nxs` v2 COMPONENT
KINDS (`boot-image-delta`, `bundle-delta`), not a new container. At execution
this task writes the normative `.nxdelta` stream-format RFC (next free number)
and implements the `boot-image-delta` kind first: reconstruct the target boot
image from the ACTIVE slot bytes + delta stream into the inactive slot, then
TASK-0179's identical digest/readback/NXBD tail runs — the engine, trust and
staging paths are untouched (that is the component-model payoff). The
rollsum+zstd mechanics, determinism, resume-checkpoint and bounded-memory
requirements below carry over verbatim; the bundlemgrd `bundle-delta` apply path
moves BEHIND the Phase-B bundle-set seam (RFC-0089 §12) and executes with it.
depends-on at execution: TASK-0179 (real apply path first). Fixture emission:
`nx image ota --delta-from <img>` (TASK-0260).

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
