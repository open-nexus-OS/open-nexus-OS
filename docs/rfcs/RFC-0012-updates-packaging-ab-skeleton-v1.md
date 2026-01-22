<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0012: Updates & Packaging v1.0 — System-Set (.nxs) + userspace-only A/B skeleton (non-persistent)

- Status: Complete
- Owners: @runtime, @tools-team
- Created: 2026-01-15
- Last Updated: 2026-01-16
- Links:
  - Tasks: `tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md` (execution + proof)
  - ADRs:
    - `docs/adr/0020-manifest-format-capnproto.md` (bundle manifest contract: `manifest.nxb`)
  - Packaging docs:
    - `docs/packaging/system-set.md` (system-set layout + signature binding)
  - Related RFCs:
    - `docs/rfcs/RFC-0003-unified-logging.md` (audit logging facade discipline)
    - `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md` (os-lite dependency rules)

## Status at a Glance

- **Phase 0 (Bundle manifest contract unification)**: ✅
- **Phase 1 (System-Set format `.nxs` + builder tool `nxs-pack`)**: ✅
- **Phase 2 (Update domain library + `updated` service contract)**: ✅
- **Phase 3 (Init + slot publication integration + proof markers)**: ✅

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The `.nxs` (Nexus System-Set) contract: archive layout, `system.nxsindex` schema, determinism requirements, signature binding, and size bounds.
  - The v1.0 A/B update model contract (non-persistent): `stage → switch → health gate → rollback`.
  - The `updated` service contract (RPCs + failure model + required audit logging + marker strings).
  - Slot-aware publication contract for `bundlemgrd` (republish based on active slot; markers).
  - Proof gates (host tests + deterministic QEMU markers) required to call this milestone “Complete”.

- **This RFC does NOT own**:
  - Persistent boot control / statefs integration (TASK-0009 + TASK-0034).
  - Digest/size fields inside `manifest.nxb` (v1.1 work in TASK-0034).
  - Real reboot / bootloader/OpenSBI slot signaling (TASK-0037).
  - Delta updates / streaming / compression tuning (TASK-0034, TASK-0035).
  - Full update authorization policy (a follow-on RFC/task; v1.0 must still verify signatures and be fail-closed).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC must link to the task(s) that implement and prove each phase/milestone.

## Context

We want the first verifiable OTA story without touching the kernel or bootloader:

**Staging → Switch → Health gate → Rollback**.

Phase 0 (already accepted by ADR-0020) unified the bundle manifest format repo-wide:

- `.nxb` bundles are directories containing `manifest.nxb` (Cap’n Proto binary) and `payload.elf`.

This RFC defines the v1.0 **system-set** container `.nxs` and the userspace-only A/B skeleton that stages and activates sets (non-persistent).

## Goals

- Define a deterministic, bounded, signed system-set container (`.nxs`) that cryptographically binds the included bundles.
- Provide a minimal, host-testable A/B state machine (non-persistent) proving:
  - `stage` verifies system-set signature, validates bounds, and atomically stages bundles into the standby slot
  - `switch` flips to the staged slot (soft switch) and starts a bounded health window (`triesLeft`)
  - `health ok` commits and clears pending
  - lack of health commit triggers rollback
- Define the `updated` service contract for OS-lite, including error/failure behavior and deterministic markers.

## Non-Goals

- Persistence across reboot, statefs-backed boot control (TASK-0009 + TASK-0034).
- Real reboot behavior and boot-chain slot signals (TASK-0037).
- Delta updates, streaming, compression (TASK-0034, TASK-0035).
- Any scheme that skips signature verification “for tests” (hard prohibited).

## Constraints / invariants (hard requirements)

- **Determinism**:
  - `.nxs` generation is deterministic (stable ordering, stable serialization).
  - QEMU markers are stable strings; no timestamps/random IDs in markers.
  - Tests are bounded (no infinite loops/unbounded waits).

- **No fake success**:
  - `updated: ready (...)` only after the service can actually verify and operate.
  - `SELFTEST: ... ok` only after actual state transitions.

- **Bounded resources**:
  - `.nxs` archive size MUST be checked and capped before extraction.
  - Per-entry sizes MUST be bounded before allocation/IO.
  - Extraction MUST reject path traversal and unsafe paths.
  - Stage requests are capped to `MAX_STAGE_BYTES` (8 KiB) by the kernel IPC v1 frame limit; larger system-sets require a follow-on handle/path-based stage API.

- **Security floor**:
  - System-set signature verification is mandatory; no bypass.
  - Bundle bytes MUST be cryptographically bound to the system-set signature via digests in `system.nxsindex`.
  - Never log secrets (private keys). Digests and signatures are OK.

- **OS build hygiene**:
  - OS services MUST build with `--no-default-features --features os-lite`.
  - Forbidden crates MUST NOT enter the OS dependency graph (`just dep-gate` is authoritative).

## Proposed design

### Terminology

- **Slot**: `a` or `b`.
- **Active slot**: slot currently published/served by the system.
- **Pending slot**: slot staged and selected for activation but not yet committed healthy.
- **triesLeft**: bounded integer counter decremented per boot attempt while pending (v1.0 models this in-memory).
- **Health commit**: explicit signal from init that the system reached stable state.

### Bundle contract (normative via ADR-0020)

An `.nxb` bundle is a directory:

- `<bundle>.nxb/manifest.nxb` (Cap’n Proto binary; schema per `tools/nexus-idl/schemas/manifest.capnp`)
- `<bundle>.nxb/payload.elf` (ELF payload bytes)

This RFC does not redefine the manifest schema; ADR-0020 is authoritative.

### System-Set container: `.nxs` (normative)

#### Archive format

`.nxs` is a tar archive with the following layout:

```text
system.nxsindex
system.sig.ed25519
<bundle-name-1>.nxb/
  manifest.nxb
  payload.elf
<bundle-name-2>.nxb/
  manifest.nxb
  payload.elf
...
```

#### Deterministic ordering

- `system.nxsindex` MUST be the first tar entry.
- `system.sig.ed25519` MUST be the second tar entry.
- Bundle directories MUST be ordered by `<bundle-name>` in lexicographic byte order.
- Within each bundle directory, entries MUST be ordered:
  1) `manifest.nxb`
  2) `payload.elf`

#### Size bounds (normative; v1.0 defaults, may be tightened)

Implementations MUST enforce explicit maxima and reject on exceed:

- `MAX_NXS_ARCHIVE_BYTES` (default: 100 MiB)
- `MAX_SYSTEM_NXSINDEX_BYTES` (default: 1 MiB)
- `MAX_MANIFEST_NXB_BYTES` (default: 256 KiB)
- `MAX_PAYLOAD_ELF_BYTES` (default: 50 MiB per bundle)
- `MAX_BUNDLES_PER_SET` (default: 256)

#### Path safety (normative)

Tar entries MUST be rejected if any path:

- is absolute
- contains `..`
- contains NUL
- escapes the logical root

#### `system.nxsindex` schema (normative)

`system.nxsindex` is a **Cap’n Proto binary** (deterministic, directly signable bytes).

The schema MUST be defined in one place (single truth): `tools/nexus-idl/schemas/system-set.capnp`.

Logical structure:

```text
schemaVersion: UInt8 = 1
systemVersion: Text (SemVer)
publisher: Data (32 bytes)
timestampUnixMs: UInt64 (metadata; MUST NOT be used in markers)
bundles: List(BundleEntry)
  - name: Text
  - version: Text (SemVer)
  - manifestSha256: Data (32 bytes; SHA-256 over manifest.nxb bytes)
  - payloadSha256: Data (32 bytes; SHA-256 over payload.elf bytes)
  - payloadSize: UInt64
```

Rules:

- `schemaVersion` MUST be `1`.
- `timestampUnixMs` MAY be present but MUST NOT be used in markers (determinism).
- `manifestSha256` is SHA-256 over the raw `manifest.nxb` bytes.
- `payloadSha256` / `payloadSize` are over `payload.elf`.

#### Signature binding (normative)

- The Ed25519 signature MUST be computed over the raw bytes of `system.nxsindex`.
- The signature MUST be stored as exactly 64 bytes in `system.sig.ed25519`.
- Verification MUST:
  - read `system.nxsindex` bytes,
  - read `system.sig.ed25519` bytes,
  - call `keystored.verify(pubkey, system_nxsindex_bytes, signature)` (verify-only; no private key custody on device).

### Update state machine (v1.0 non-persistent; normative)

`BootCtrl` is a state machine with the following contract:

State:

- `active_slot: Slot`
- `pending_slot: Option<Slot>`
- `tries_left: u8` (bounded)
- `health_ok: bool` (for the pending slot)

Operations:

- `stage(system_set)`:
  - verifies signature of `system.nxsindex`
  - verifies all bundle digests match the bytes in the archive
  - stages extracted bundles into the standby slot atomically
- `switch()`:
  - sets `pending_slot = standby_slot`
  - sets `tries_left = N` (fixed, deterministic, small; default `N=2`)
  - triggers slot-aware publication via `bundlemgrd.set_active_slot(pending_slot)`
- `health_ok()`:
  - commits: `active_slot = pending_slot`, clears `pending_slot`, sets `health_ok = true`
- `on_boot_attempt()` (modeled by init in v1.0):
  - if `pending_slot.is_some()`: decrement `tries_left`
  - if `tries_left == 0` and still pending: rollback to previous active slot
- `rollback()`:
  - clears `pending_slot`, sets `tries_left = 0`, and re-publishes previous active slot

v1.0 persistence model:

- Boot control state is RAM-only; it MAY be serialized to a tmpfs-like path, but MUST be treated as non-durable.

### `updated` service contract (normative)

`updated` is the OS-lite daemon responsible for:

- validating/staging system-sets
- coordinating slot switching and health commit
- emitting audit logs and deterministic markers

RPCs (conceptual; concrete IPC framing is task-owned):

- `StageSystemSet(nxs_bytes_or_handle) -> Result<()>`
- `Switch() -> Result<()>`
- `HealthOk() -> Result<()>`
- `BootAttempt() -> Result<RollbackSlot?>` (decrements tries-left; returns rollback if triggered)
- `GetStatus() -> Status` where `Status` includes `active_slot`, `pending_slot`, `tries_left`.

Failure behavior (normative):

- MUST fail closed. No “warn and continue” on verification failures.
- MUST return explicit errors for:
  - invalid/absent signature
  - oversized archive / oversized entry
  - digest mismatch
  - malformed tar / unsafe paths

Required marker strings (normative):

- `updated: ready (non-persistent)`
- (Selftest emits):
  - `SELFTEST: ota stage ok`
  - `SELFTEST: ota switch ok`
  - `SELFTEST: ota rollback ok`

Audit logging (normative):

- Every operation MUST emit a structured audit record via the logging facade (scope=`updated`):
  - op ∈ {stage, switch, health_ok, rollback}
  - slot, result, and (allowed) metadata like digest hex (never private keys)

### Init + bundle publication integration (normative)

- Init MUST:
  - on boot, query/update the v1.0 boot control state (RAM model)
  - if pending: decrement tries and decide rollback when exhausted
  - after core services are up and selftest completes, call `updated.HealthOk()` and emit:
    - `init: health ok (slot <a|b>)`

- `bundlemgrd` MUST:
  - provide an operation to set active slot (soft switch)
  - republish bundles from `/system/<slot>/` into its publish view
  - emit: `bundlemgrd: slot <a|b> active` only after republication completes

**Current state (bring-up note)**:

- `init` issues a `BootAttempt()` on boot and applies rollback via `bundlemgrd` if signaled.
- `init` receives a health-ok signal from `selftest-client` and forwards `HealthOk()` to `updated`.
- `bundlemgrd` os-lite encodes the active slot in `build.prop` and version suffix (e.g., `1.0.0-a`).

## Security considerations

### Threat model

- **Malicious update injection**: Attacker tampers with `.nxs` during transport
- **Signature bypass**: Unsigned or forged system-sets
- **Integrity failure**: Bundle bytes replaced after verification
- **Downgrade/rollback**: Reversion to vulnerable versions (future hardening; v1.0 is fail-closed)
- **Health-check manipulation**: Auto-commit without real health verification

### Security invariants

- `.nxs` MUST be signed; signature MUST be verified before staging
- Bundle bytes staged MUST match digests from `system.nxsindex`
- Staging MUST be atomic (no partial visibility)
- All update operations MUST be audit logged to `logd`
- All untrusted inputs MUST be size-bounded before allocation/processing

### DON'T DO

- Don't skip signature verification even for tests/localhost
- Don't "warn and continue" on verification failures
- Don't store private signing keys on device
- Don't accept unbounded archives/entries
- Don't attempt "userspace-only anti-rollback" in v1.0: without persistence + boot-chain anchoring it is security theater

### Policy enforcement (out of scope for v1.0)

Signature verification is mandatory for v1.0. The enforcement modes, trust-store policy,
and developer workflow are defined in `docs/security/signing-and-policy.md` (single source of truth).

**v1.0 implementation note**:

- `updated` enforces signature verification for all builds.
- Trust-store checks and policy routing are deferred until `policyd`/`keystored` enforcement wiring lands.

### Downgrade / rollback protection (explicit non-goal for v1.0; security note)

Android-grade anti-rollback requires a **persistent, monotonic source** anchored in the boot chain (e.g. verified boot rollback indices).

In Open Nexus OS, downgrade protection MUST be deferred until we have:

- persistence via statefs (TASK-0009), and
- boot-chain / slot signal integration (TASK-0037), and/or a hardware-backed monotonic counter.

Until then, v1.0 MUST remain fail-closed on signature/integrity but MUST NOT claim downgrade protection.

## Failure model (normative)

- If `.nxs` signature verification fails: stage MUST fail; no partial staging; no slot changes.
- If any bundle digest mismatch: stage MUST fail; no partial staging.
- If `.nxs` is oversized or contains oversized entries: reject deterministically.
- If health is not committed before `tries_left` reaches 0: system MUST rollback to last known active slot and republish it.

## Proof / validation strategy (required)

### Proof (Host)

Canonical host tests must cover:

- stage → switch → health_ok commit
- reject: unsigned system-set
- reject: invalid signature
- reject: digest mismatch (manifest or payload)
- reject: oversized archive/entry
- rollback when health isn’t committed in time/tries

Canonical command (task defines exact crate/path):

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace updates_host
```

### Proof (OS/QEMU)

Canonical proof is marker-driven:

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Required markers:

- `updated: ready (non-persistent)`
- `bundlemgrd: slot a active` (initial)
- `SELFTEST: ota stage ok`
- `SELFTEST: ota switch ok`
- `init: health ok (slot <a|b>)`
- `SELFTEST: ota rollback ok`

## Alternatives considered

- Sign the entire `.nxs` tar bytes: simpler binding but harder for streaming/inspection. Rejected for v1.0.
- JSON system index (canonical JSON): rejected in v1.0 because canonicalization becomes part of the security contract. We choose a Cap’n Proto binary index (`system.nxsindex`) so the signed bytes are unambiguous.
- No per-bundle digests: rejected because signature would not bind bundle bytes (integrity gap).
- Real reboot-based switching: out of scope (boot chain integration required).

## Open questions

- Cap’n Proto index evolution strategy: versioning policy for `system-set.capnp` (e.g., additive fields + explicit schemaVersion gating).
- Update authorization policy: which services are allowed to call `updated` operations; likely route via `policyd` in a follow-on task/RFC.
- Key distribution model for publisher keys (provisioning/rotation).

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

- [x] **Phase 0**: Bundle manifest unification (`manifest.nxb`) — ADR-0020 accepted.
- [x] **Phase 1**: System-Set `.nxs` format + `nxs-pack` tool — `cargo build -p nxs-pack`
- [x] **Phase 2**: `updated` service + domain library — `cargo test -p updates_host`
- [x] **Phase 3**: Init/bundlemgrd slot integration + QEMU markers — `just test-os`
- [x] Task linked: `tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md`
- [x] QEMU markers pass: `SELFTEST: ota stage ok`, `SELFTEST: ota switch ok`, `SELFTEST: ota rollback ok`
- [x] Security negative tests: `test_reject_invalid_signature`, `test_reject_digest_mismatch`
