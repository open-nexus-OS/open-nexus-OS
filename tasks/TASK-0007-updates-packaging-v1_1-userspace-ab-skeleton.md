---

title: TASK-0007 Updates & Packaging v1.0 (OS): userspace-only A/B skeleton (non-persistent) + manifest.nxb unification
status: Draft
owner: @runtime
created: 2025-12-22
updated: 2026-01-15
links:

  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Testing contract: scripts/qemu-test.sh
  - Packaging docs: docs/packaging/nxb.md
  - ADR: docs/adr/0020-manifest-format-capnproto.md
  - ADR: docs/adr/0009-bundle-manager-architecture.md
  - Rust standards: docs/standards/RUST_STANDARDS.md
  - Security standards: docs/standards/SECURITY_STANDARDS.md

follow-up-tasks:

  - TASK-0034: Delta updates v1 + v1.1 features (digest/size fields, persistence integration)
  - TASK-0035: Delta updates v1b (system-set level)
  - TASK-0037: OTA A/B v2 (bootloader/SBI integration for real reboot)
  - TASK-0140: Updates v1 UI/CLI (Settings UI + nx update command)
  - TASK-0179: Updated v2 (offline feed, delta, health, rollback refinements)
  - TASK-0178: Bootctld v1 (boot control stub service for A/B coordination)
  - TASK-0009: Persistence v1 (enables persistent bootctl, unblocks TASK-0034)
---

## Context

We want the first verifiable OTA story without touching the kernel or bootloader:

**Staging → Switch → Health gate → Rollback**.

The goal is a **userspace skeleton** that is testable and honest:

- "switch" updates a userspace-controlled boot target (`bootctl.json` in RAM) and forces slot-aware publication,

  even if we cannot perform a real reboot yet.

- health is explicitly committed by userspace once the system is stable.
- rollback happens automatically when pending boots do not reach health.

**v1.0 scope (this task)**:

- **Non-persistent**: `bootctl.json` lives in RAM only (no survival across real reboots)
- **Manifest unification**: Resolve 3-way format drift (JSON/TOML/nxb) → single Cap'n Proto binary
- **System-set format**: Define `.nxs` archive + signed index
- **Proof**: Stage/switch/health/rollback work in-process (QEMU markers)

**v1.1 scope (moved to TASK-0034)**:

- **Persistent bootctl**: Integrate with TASK-0009 (statefs)
- **Per-bundle digest/size**: Add fields to manifest.nxb schema
- **Real reboot testing**: Prove persistence across VM restart

Prompt mapping note (avoid drift from older plans):

- Some older prompts describe a "NUB" update bundle with `manifest.json` + `payload.tar` + `sig.ed25519`.
- This repo's v1.0 decision is **not** to add a parallel JSON+tar update container.
- Canonical contracts here are:
  - **`.nxb`** bundle with **`manifest.nxb`** (Cap'n Proto binary) + `payload.elf` (see ADR-0020)
  - a **system-set** archive/index (`.nxs`) that is signed and lists bundles+digests.

## Goal

Prove end-to-end (host tests + QEMU selftest markers):

- A system-set can be staged into the standby slot after verifying signatures.
- Switching sets `pending` + `triesLeft` and flips the active slot selection (in RAM).
- A health commit clears `pending` for the new slot.
- A forced "no health" path triggers rollback back to the previous slot.
- **Manifest format is unified**: `manifest.nxb` (Cap'n Proto) is the single source of truth repo-wide.

## Non-Goals

- Real persistent storage (block backend) and real reboot integration (TASK-0009 + TASK-0034).
- Per-bundle digest/size verification (moved to TASK-0034 v1.1).
- Delta updates, streaming updates, compression tuning (TASK-0034, TASK-0035).
- Full policy language for updates (only minimal gating needed for the skeleton).
- User-facing Settings UI and a developer CLI (`nx update`) (follow-up `TASK-0140`).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **No fake success**: OTA markers must be emitted only after real verification/state transitions.
- **Determinism**: staging/switch/rollback must be deterministic; tests must be bounded.
- **Bounded data**: system-set parsing and extraction must be size-capped; reject oversized inputs.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (resolved in this task)**:
  - **Bundle manifest contract drift (RESOLVED)**:

    ✅ **Decision documented in ADR-0020**: `manifest.nxb` (Cap'n Proto binary) is the single source of truth.

    Canonical v1.0 contract for `.nxb`:

    - **`manifest.nxb`**: Cap'n Proto binary (deterministic, signable, versionable)
    - **`payload.elf`**: ELF payload bytes

    Tooling flow:

    - Input: `manifest.toml` (human-editable)
    - Compile: `nxb-pack` → `manifest.nxb`
    - Parse: `bundlemgr` (host) + `bundlemgrd` (OS)

    This task implements the unification repo-wide.

  - **Update payload format drift (RESOLVED)**:

    ✅ **Decision**: System-set (`.nxs`) with signed JSON index + tar archive.

    - No `manifest.json` + tar parallel format.
    - `.nxs` is the canonical update container.
- **YELLOW (addressed in v1.0)**:
  - **"No bootloader changes" means no real reboot (ADDRESSED)**:

    ✅ **Decision**: "Soft switch" (Option A) - IPC call to `bundlemgrd.SET_ACTIVE_SLOT()` triggers in-process republication.

    - Marker: `bundlemgrd: slot b active` after republication completes.
    - Real reboot integration deferred to TASK-0037.
  
  - **Persistence reality (ADDRESSED)**:

    ✅ **Decision**: v1.0 uses RAM-based `/state` (explicit non-persistent).

    - Marker: `updated: ready (non-persistent)`
    - Tests: In-memory only, no reboot cycle
    - Persistent bootctl moved to TASK-0034 (after TASK-0009)
  
  - **Signature verification in OS builds (ADDRESSED)**:

    ✅ **Decision**: Delegate to `keystored.verify()` API (already exists, no_std-friendly).

    - `updated` calls `keystored` for Ed25519 verification.
  
  - **Boot-chain contract (DEFERRED)**:

    Real boot-time slot signal (bootloader/OpenSBI) tracked in TASK-0037.

- **GREEN (confirmed assumptions)**:
  - `bundlemgrd`/`packagefsd` already serve bundle payloads and manifests in OS-lite flows; we can extend that model slot-aware.

## Security considerations

### Threat model

- **Malicious update injection**: Attacker stages a tampered system-set with malicious bundles
- **Signature bypass**: Attacker provides update without valid signature or with forged signature
- **Rollback attack**: Attacker forces rollback to vulnerable older version
- **Supply chain attack**: Compromised build system produces signed but malicious bundles
- **Downgrade attack**: Attacker installs older, vulnerable version with valid signature
- **Health-check manipulation**: Attacker tricks system into marking bad update as healthy

### Security invariants (MUST hold)

- System-set MUST be signed; signature MUST be verified before staging
- Per-bundle digests MUST be verified against manifest before installation
- Rollback MUST NOT be possible to versions older than a defined security baseline
- Health commit MUST only occur after genuine system stability (not just timeout)
- Staging MUST be atomic: partial/corrupted updates MUST NOT be bootable
- All update operations MUST be audit-logged

### DON'T DO

- DON'T stage updates without signature verification
- DON'T skip per-bundle digest verification
- DON'T allow unlimited rollback depth (enforce minimum version floor)
- DON'T auto-commit health without verifiable stability criteria
- DON'T store signing keys on the device (verify-only)
- DON'T accept updates from unauthenticated sources

### Attack surface impact

- **Significant**: Update system is a critical attack vector for persistent compromise
- **Supply chain risk**: Compromised updates can persist across reboots
- **Requires security review**: Any changes to signature verification must be reviewed

### Mitigations

- Ed25519 signature on system-set index (verified before staging)
- SHA-256 per-bundle digests in manifest (verified before install)
- `triesLeft` counter limits boot attempts before auto-rollback
- Version floor (minimum acceptable version) prevents downgrade attacks (future)
- Audit log of all stage/switch/health/rollback operations

## Security proof

### Audit tests (negative cases)

- Command(s):
  - `cargo test -p updates_host -- reject --nocapture`
- Required tests:
  - `test_reject_invalid_signature` — bad signature → stage fails
  - `test_reject_digest_mismatch` — tampered bundle → stage fails
  - `test_reject_missing_signature` — unsigned update → rejected
  - `test_audit_update_operations` — all ops logged

### Hardening markers (QEMU)

- `updated: stage rejected (signature)` — signature verification works
- `updated: stage rejected (digest)` — digest verification works
- `updated: audit (op=stage status=ok/fail)` — audit trail

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **Packaging contract (today)**:
  - `.nxb` layout is documented in `docs/packaging/nxb.md` (needs alignment with code as part of this task).

## Stop conditions (Definition of Done)

### Proof (Host)

- Add a deterministic host test crate (e.g. `tests/updates_host`) that covers:
  - stage verifies signature + digests (and rejects mismatch)
  - switch flips target + sets pending + triesLeft
  - health commit clears pending
  - rollback triggers when triesLeft reaches 0 without health

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend `scripts/qemu-test.sh` expected markers (order tolerant) with:
    - `updated: ready`
    - `bundlemgrd: slot a active` (or `b`, depending on initial state)
    - `SELFTEST: ota stage ok`
    - `SELFTEST: ota switch ok`
    - `init: health ok (slot <a | b>)`
    - `SELFTEST: ota rollback ok`

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.
- Update payload containers in this repo should follow the system-set direction from this task (no zip/manifest.json “NUP” contract without an explicit format decision).

### Docs gate (keep architecture entrypoints in sync)

- If packaging/manifest contract, init orchestration for updates, or bundle publication semantics change, update:
  - `docs/architecture/09-nexus-init.md`
  - `docs/architecture/15-bundlemgrd.md`
  - `docs/architecture/04-bundlemgr-manifest.md`
  - `docs/architecture/07-contracts-map.md`
  - and the index `docs/architecture/README.md`

## Touched paths (allowlist)

- `tools/nxb-pack/` (extend manifest output to v1.1 once contract chosen)
- `tools/` (add an `nxs-pack` tool for system-set creation/signing)
- `source/services/updated/` (new service: stage/switch/health API + marker)
- `source/init/nexus-init/` (boot target selection + tries/rollback + health commit)
- `source/services/bundlemgrd/` (slot-aware publication; marker)
- `source/apps/selftest-client/` (stage/switch/rollback markers)
- `scripts/qemu-test.sh` (canonical marker contract update)
- `docs/` (updates + packaging + testing docs)

## Plan (small PRs)

### Phase 1: Manifest Unification (RED flag resolution)

1. **Define Cap'n Proto schema**
   - ✅ ADR-0020 created
   - Create `tools/nexus-idl/schemas/manifest.capnp`
   - Generate Rust bindings: `capnp compile -o rust manifest.capnp`

2. **Update `nxb-pack` tool**
   - Input: `manifest.toml` (TOML, human-editable)
   - Output: `manifest.nxb` (Cap'n Proto binary)
   - Remove old `manifest.json` output

3. **Update `bundlemgr` parser (host)**
   - Replace TOML parser with Cap'n Proto parser
   - Keep `Manifest` struct API unchanged (internal only)

4. **Update `bundlemgrd` parser (OS)**
   - Add Cap'n Proto parser for OS-lite mode
   - Remove TOML parser dependency

5. **Migrate test fixtures**
   - `exec-payloads`: Compile `hello.manifest.toml` → `hello.manifest.nxb` at build time
   - Update all test bundles to use `manifest.nxb`

6. **Update docs**
   - `docs/packaging/nxb.md`: Document Cap'n Proto format
   - `docs/architecture/04-bundlemgr-manifest.md`: Update to reflect binary format
   - Update all tasks referencing manifest format

### Phase 2: System-Set Format + Tooling

1. **Define `.nxs` format**
   - Document `system.json` schema
   - Define tar archive structure

2. **Create `nxs-pack` tool**
   - Input: List of `.nxb` bundles
   - Output: `system-vX.Y.Z.nxs` (signed tar archive)

### Phase 3: `updated` Service

1. **Implement `userspace/updates` (host-first)**
   - `BootCtrl` struct (RAM-based, non-persistent)
   - `SystemSet` parser
   - Signature verification (via `keystored` client)

2. **Implement `updated` service (OS-lite)**
   - RPC: `StageSystem`, `Switch`, `HealthOk`
   - Marker: `updated: ready (non-persistent)`

### Phase 4: Init + bundlemgrd Integration

1. **Init integration (health gate + rollback)**
   - Early boot: Read RAM-based `bootctl` state
   - Rollback logic: `triesLeft` decrement
   - Health commit: Call `updated.HealthOk()`
   - Marker: `init: health ok (slot <a | b>)`

2. **bundlemgrd slot-aware publication**
   - Add `OP_SET_ACTIVE_SLOT` RPC
   - Republish bundles from `/system/<slot>/`
   - Marker: `bundlemgrd: slot <a | b> active`

### Phase 5: Selftest + Docs

1. **Selftest proof**
   - Happy path: stage + switch + health
   - Rollback path: force pending without health
   - Markers: All 6 OTA markers present

2. **Documentation**
   - `docs/updates/ab-skeleton-v1.md`: v1.0 model (non-persistent)
   - Update architecture docs
   - Update testing docs

## Acceptance criteria (behavioral)

- Host tests cover stage/switch/health/rollback and negative cases deterministically.
- QEMU run prints the OTA markers listed above and `scripts/qemu-test.sh` passes.
- No kernel changes.

## Evidence (to paste into PR)

- Host: `cargo test -p updates_host -- --nocapture` summary
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` + `uart.log` tail with OTA markers

## RFC seeds (for later, once green)

- Decisions made:
  - canonical bundle manifest format + v1.1 fields
  - system-set archive format and signature scheme
  - meaning of “switch” without reboot (soft switch vs state-only)
- Open questions:
  - persistence backend and reboot integration
  - policy model for who may stage/switch/rollback
