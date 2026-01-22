# TASK-0007 Handoff: Updates & Packaging v1.0

**Date**: 2026-01-15  
**Status**: Ready for implementation (Phase 0 complete)  
**Context**: Repo-wide manifest unification complete, all drift resolved

---

## Executive Summary

You are implementing **TASK-0007 v1.0: Updates & Packaging (OS) - userspace-only A/B skeleton (non-persistent) + manifest.nxb unification**.

**Critical context**: Phase 0 (Decisions & Unification) is **DONE**. The manifest format is now unified repo-wide to `manifest.nxb` (Cap'n Proto binary). You are starting **Phase 1: Tooling** (System-Set format + `nxs-pack` tool).

**Scope**: v1.0 is **non-persistent** (RAM-based bootctl). v1.1 features (persistent bootctl, digest/size fields) have been moved to TASK-0034.

---

## Must-Read Files (in order)

### 1. Project Standards & Agent Guidelines

**Read these FIRST before touching any code:**

1. **`docs/agents/PLAYBOOK.md`** - Agent workflow, session rules, task execution truth
2. **`docs/agents/VISION.md`** - Project vision, architecture principles, decision framework
3. **`docs/standards/RUST_STANDARDS.md`** - Rust layering (kernel/OS/userspace), feature gates, error handling
4. **`docs/standards/SECURITY_STANDARDS.md`** - Security invariants, testing requirements, DON'T DO list
5. **`docs/standards/BUILD_STANDARDS.md`** - Feature gates, dependency hygiene, `dep-gate` enforcement
6. **`docs/standards/DOCUMENTATION_STANDARDS.md`** - CONTEXT header format, doc requirements

### 2. Task Files

**Primary task definition:**

* **`tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md`** - Main task (357 lines)
  * Read **entire file** - context, constraints, security, plan, stop conditions
  * Note: Title says "v1_1" but content is now v1.0 scope (non-persistent)
  * Phase 0 (Decisions) is DONE ✅
  * You are implementing Phase 1-5

**Related tasks:**

* **`tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`** - Enables persistent bootctl (TASK-0034)
* **`tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md`** - Contains v1.1 features moved from TASK-0007
* **`tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md`** - Bundle install/upgrade (parallel work)
* **`tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`** - Observability (DONE, use for audit logging)

### 3. Architecture & Design Decisions

**Manifest format (CRITICAL - already implemented):**

* **`docs/adr/0020-manifest-format-capnproto.md`** - ADR: Why Cap'n Proto for manifest.nxb
* **`tools/nexus-idl/schemas/manifest.capnp`** - Cap'n Proto schema definition
* **`docs/packaging/nxb.md`** - NXB bundle format documentation
* **`docs/architecture/04-bundlemgr-manifest.md`** - Bundle manifest contract

**System architecture:**

* **`docs/ARCHITECTURE.md`** - Central architecture overview (kernel, services, observability)
* **`docs/architecture/README.md`** - Architecture index (start here for deep dives)
* **`docs/architecture/05-system-map-and-boundaries.md`** - Runtime roles and boundaries
* **`docs/architecture/07-contracts-map.md`** - All system contracts
* **`docs/architecture/08-service-architecture-onboarding.md`** - Service patterns
* **`docs/adr/0017-service-architecture.md`** - ADR: Service architecture principles

**Existing services (study these patterns):**

* **`docs/architecture/14-samgrd.md`** - Service manager (registration/discovery)
* **`docs/architecture/15-bundlemgrd.md`** - Bundle manager (install/query)
* **`docs/architecture/10-execd-and-loader.md`** - Execution daemon (crash reporting)

### 4. Testing & Proof Requirements

* **`docs/testing/index.md`** - Testing methodology, QEMU markers, coverage matrix
* **`docs/testing/e2e-coverage-matrix.md`** - E2E test coverage strategy
* **`scripts/qemu-test.sh`** - QEMU test harness (marker sequence)

---

## What Has Been Done (Phase 0)

### ✅ Manifest Format Unification (Complete)

**Decision**: `manifest.nxb` (Cap'n Proto binary) is now the **single source of truth** repo-wide.

**Implemented**:

1. **Schema defined**: `tools/nexus-idl/schemas/manifest.capnp`
   * `BundleManifest` struct with all required fields
   * Version 1 (v1.0): name, semver, abilities, caps, minSdk, publisher, signature
   * Version 1.1 fields (for TASK-0034): payloadDigest, payloadSize

2. **Tooling updated**:
   * `tools/nxb-pack/src/main.rs` - Now outputs `manifest.nxb` (binary)
   * Accepts `--toml` flag for human-editable input
   * `userspace/exec-payloads/build.rs` - Generates `.nxb` at build time

3. **Parsers migrated**:
   * `userspace/bundlemgr/src/manifest.rs` - Parses Cap'n Proto binary
   * `source/services/bundlemgrd/src/std_server.rs` - Uses binary manifest
   * `source/services/packagefsd/src/os_lite.rs` - Publishes `manifest.nxb`

4. **Tests updated**:
   * `userspace/bundlemgr/tests/manifest_tests.rs` - Cap'n Proto test suite
   * `tests/vfs_e2e/tests/vfs_roundtrip.rs` - Uses `manifest.nxb`
   * `tests/e2e_policy/src/lib.rs` - Binary manifest helpers
   * `tests/e2e/tests/host_roundtrip.rs` - Binary manifest tests
   * `tests/remote_e2e/tests/remote.rs` - Remote bundle install with binary

5. **Documentation updated**:
   * `docs/adr/0020-manifest-format-capnproto.md` - ADR created
   * `docs/packaging/nxb.md` - Updated to reflect binary format
   * `docs/architecture/04-bundlemgr-manifest.md` - Unified format documented

**Result**: No more "three truths" (JSON/TOML/NXB). Single binary format everywhere.

### ✅ Task Drift Resolved (Complete)

**Circular dependency broken**:

* TASK-0007 v1.0: Non-persistent A/B skeleton + manifest unification
* TASK-0034 v1.1: Persistent bootctl + digest/size fields (depends on TASK-0009)

**Dependencies clarified**:

* TASK-0007 v1.0 does NOT depend on TASK-0009 (persistence)
* TASK-0034 v1.1 DOES depend on TASK-0009 (for persistent bootctl)

---

## What You Need to Implement (Phase 1-5)

### Phase 1: System-Set Format Definition

**Goal**: Define `.nxs` (Nexus System-Set) format for OTA payloads.

**Tasks**:

1. **Document `system.nxsindex` schema**:
   * Cap'n Proto binary index (single truth; deterministic bytes)
   * List of bundles with names, versions, digests
   * System-wide metadata (version, timestamp, publisher)
2. **Document signature binding**:
   * `system.sig.ed25519` is a detached Ed25519 signature over the raw `system.nxsindex` bytes

3. **Define tar archive structure**:
   * `system.nxsindex` (unsigned index) + `system.sig.ed25519` (detached signature)
   * `<bundle-name>.nxb` (one per bundle)
   * Deterministic tar ordering (for reproducibility)

**Deliverables**:

* `docs/packaging/system-set.md` - Format specification
* `tools/nexus-idl/schemas/system-set.capnp` - Cap'n Proto schema (single truth for `system.nxsindex`)

### Phase 2: `nxs-pack` Tool

**Goal**: Create tool to build `.nxs` archives from `.nxb` bundles.

**Implementation**:

* `tools/nxs-pack/src/main.rs` - New binary crate
* Input: Directory of `.nxb` files + metadata TOML
* Output: `system-vX.Y.Z.nxs` (signed tar archive)
* Signature: Call `keystored` or use Ed25519 crate directly

**Example usage**:

```bash
nxs-pack --input bundles/ --meta system-set.toml --key keys/ed25519.hex --output system-v1.0.0.nxs
```

### Phase 3: `userspace/updates` Library

**Goal**: Host-testable update logic (RAM-based bootctl).

**Implementation**:

* `userspace/updates/src/lib.rs` - Domain library
* `BootCtrl` struct (RAM-based, non-persistent)
  * Fields: `active_slot`, `pending_slot`, `tries_left`, `health_ok`
  * Methods: `stage()`, `switch()`, `commit_health()`, `rollback()`
* `SystemSet` parser (for `.nxs` archives)
* Signature verification (via `keystored` client or direct Ed25519)

**Feature gates**:

```toml
[features]
default = ["std"]
std = []
os-lite = []
```

**Tests**:

* `tests/updates_host/` - Host E2E tests
* Positive: stage → switch → health → commit
* Negative: `test_reject_unsigned`, `test_reject_invalid_digest`, `test_rollback_on_health_timeout`

### Phase 4: `updated` Service

**Goal**: OS-lite daemon exposing update RPCs.

**Implementation**:

* `source/services/updated/src/main.rs` - Service daemon
* `source/services/updated/src/os_lite.rs` - OS-lite backend
* RPCs: `StageSystem`, `Switch`, `HealthOk`, `GetStatus`
* Marker: `updated: ready (non-persistent)`

**IDL** (optional, for host mode):

* `tools/nexus-idl/schemas/updated.capnp` - Cap'n Proto schema

**Integration**:

* Register with `samgrd` on startup
* Use `userspace/updates` (compiled with `nexus_env="os"`)
* Emit audit logs to `logd` (scope=`updated`, structured fields)

### Phase 5: Init + bundlemgrd Integration

**Goal**: Health gate + rollback logic in init, slot-aware publication in bundlemgrd.

**Init changes** (`source/apps/init/src/main.rs`):

1. Early boot: Read RAM-based bootctl state
2. Decrement `tries_left` if pending slot
3. Wait for core markers + selftest end
4. Call `updated.HealthOk()` if all green
5. Emit: `init: health ok (slot <a|b>)`
6. If `tries_left == 0`, trigger rollback

**bundlemgrd changes** (`source/services/bundlemgrd/src/std_server.rs`):

1. Add `OP_SET_ACTIVE_SLOT` RPC
2. Republish bundles from `/system/<slot>/` to VFS
3. Emit: `bundlemgrd: slot <a|b> active`

### Phase 6: Selftest Proof

**Goal**: Deterministic QEMU markers proving OTA flow.

**New markers** (add to `scripts/qemu-test.sh`):

1. `updated: ready (non-persistent)`
2. `bundlemgrd: slot a active`
3. `SELFTEST: ota stage ok`
4. `SELFTEST: ota switch ok`
5. `init: health ok (slot <a|b>)`
6. `SELFTEST: ota rollback ok`

**Selftest client changes** (`source/apps/selftest-client/src/main.rs`):

* Add OTA test cases (stage/switch/health/rollback)
* Use `updated` client to trigger operations
* Verify bootctl state via `GetStatus` RPC

### Phase 7: Documentation

**Required docs**:

1. **`docs/updates/ab-skeleton-v1.md`** - v1.0 model (non-persistent)
2. Update **`docs/architecture/09-nexus-init.md`** - Health gate logic
3. Update **`docs/architecture/15-bundlemgrd.md`** - Slot-aware publication
4. Update **`docs/architecture/07-contracts-map.md`** - Add `updated` contract
5. Update **`README.md`** - Mention OTA v1.0

---

## Critical Constraints & Invariants

### Security (from SECURITY_STANDARDS.md)

**MUST**:

* Verify signature on every `.nxs` system-set (no exceptions)
* Verify digest on every `.nxb` bundle (if present in manifest v1.1)
* Atomic staging (no partial updates visible to system)
* Audit log ALL update operations (stage/switch/health/rollback) to `logd`
* Bounded input sizes (no unbounded reads from `.nxs` archives)

**DON'T**:

* DON'T skip signature verification even for localhost/test builds
* DON'T use "warn and continue" on signature/digest failures
* DON'T store private keys on device (use `keystored` for verification only)
* DON'T accept unbounded `.nxs` archive sizes
* DON'T log secrets (digests are OK, signatures are OK, keys are NOT)

**Negative tests required**:

* `test_reject_unsigned_system_set`
* `test_reject_invalid_signature`
* `test_reject_mismatched_digest`
* `test_reject_oversized_archive`
* `test_rollback_on_health_timeout`

### Build Hygiene (from BUILD_STANDARDS.md)

**OS services MUST**:

* Use `--no-default-features --features os-lite` for RISC-V target
* Run `just dep-gate` before every commit (fails on forbidden crates)
* Forbidden: `parking_lot`, `parking_lot_core`, `getrandom`

**Always run before commit**:

```bash
just dep-gate && just diag-os  # OS build check
just fmt-check                  # Formatting
just lint                       # Clippy
just test-host                  # Host tests
just test-os                    # QEMU tests
```

### Rust Standards (from RUST_STANDARDS.md)

**Layering**:

* Kernel: Untouched in TASK-0007 ✅
* Core libraries: `userspace/updates` (host-first, `#![forbid(unsafe_code)]`)
* OS services: `updated` (thin adapter over `userspace/updates`)

**Error handling**:

* No `unwrap`/`expect` in daemons (propagate errors with context)
* Use `Result<T, E>` for all fallible operations
* Log errors before returning (scope=`updated`, level=ERROR)

**Newtypes** (recommended):

* `SlotId(u8)` instead of raw `u8`
* `TriesLeft(u8)` instead of raw `u8`
* `SystemSetDigest([u8; 32])` instead of raw array

### Determinism (from PLAYBOOK.md)

**MUST**:

* All tests bounded (no infinite loops, no unbounded waits)
* UART markers deterministic (no random IDs, no timestamps in markers)
* QEMU tests exit on marker sequence (see `scripts/qemu-test.sh`)

**Markers format**:

```text
updated: ready (non-persistent)
bundlemgrd: slot a active
SELFTEST: ota stage ok
SELFTEST: ota switch ok
init: health ok (slot a)
SELFTEST: ota rollback ok
```

---

## File Protection Zones

### PROTECTED (NEVER modify without explicit approval)

* `Makefile` - Root build orchestrator
* `.cargo/config.toml` - Toolchain/target defaults
* `Cargo.toml` - Workspace definition (root)
* `config/` - Lint/deny/rustfmt/toolchain configs
* `scripts/` - Build/run/check scripts
* `docs/rfcs/` - Design decisions (RFC process applies)
* `source/kernel/**` - Entire kernel tree
* `source/libs/**` - All core libraries (unless explicitly in "Touched paths")

### CAUTION (modify with deep thinking)

* `source/libs/nexus-abi/**` - Stable ABI types
* `source/services/samgr/**` - Service manager
* `source/services/bundlemgr/**` - Bundle manager
* `source/apps/init/**` - Init process (you WILL touch this for health gate)

### SAFE TO MODIFY (normal iteration)

* `source/services/updated/**` - NEW service (you create this)
* `userspace/updates/**` - NEW library (you create this)
* `tools/nxs-pack/**` - NEW tool (you create this)
* `docs/updates/**` - NEW docs (you create this)
* `tests/updates_host/**` - NEW tests (you create this)

---

## Development Workflow

### 1. Read Phase

**Before writing any code**:

1. Read all "Must-Read Files" (section above)
2. Read TASK-0007 in full (357 lines)
3. Read related ADRs and architecture docs
4. Study existing service patterns (samgrd, bundlemgrd, execd)

### 2. Plan Phase

**Create implementation plan**:

1. Break Phase 1-7 into small, testable slices
2. Identify dependencies (e.g., `nxs-pack` before `updated`)
3. Plan test strategy (host tests first, then QEMU)

### 3. Implement Phase

**For each slice**:

1. Write host tests first (TDD)
2. Implement domain logic in `userspace/updates`
3. Add OS-lite adapter in `source/services/updated`
4. Run `just dep-gate && just diag-os` (OS build check)
5. Run `just test-host` (host tests)
6. Add QEMU markers and update `scripts/qemu-test.sh`
7. Run `just test-os` (QEMU tests)

### 4. Documentation Phase

**For each feature**:

1. Update relevant architecture docs
2. Add examples to `docs/updates/`
3. Update `README.md` if user-facing

### 5. Proof Phase

**Before marking task complete**:

1. All stop conditions met (see TASK-0007 line 337-357)
2. All QEMU markers present and green
3. All negative tests passing
4. All documentation updated
5. `just test-all` green
6. `make test && make build && make run` green

---

## Common Pitfalls & How to Avoid Them

### 1. Scope Creep

**Problem**: Implementing v1.1 features (persistent bootctl, digest verification) in TASK-0007.

**Solution**: v1.0 is **non-persistent** (RAM-based). Persistence is TASK-0034 (after TASK-0009).

### 2. Signature Verification Shortcuts

**Problem**: Skipping signature verification for "test builds" or "localhost".

**Solution**: ALWAYS verify signatures. Use test keys, but never skip verification.

### 3. Unbounded Input

**Problem**: Reading entire `.nxs` archive into memory without size checks.

**Solution**: Check archive size BEFORE reading. Reject if > reasonable limit (e.g., 100MB).

### 4. Non-Deterministic Tests

**Problem**: QEMU tests with random IDs, timestamps, or unbounded waits.

**Solution**: All markers deterministic. Use bounded timeouts. No random data in markers.

### 5. Forgetting `dep-gate`

**Problem**: Accidentally adding forbidden crates (e.g., `parking_lot`).

**Solution**: Run `just dep-gate` before EVERY commit. CI will fail if you forget.

### 6. Missing Negative Tests

**Problem**: Only testing happy path (stage → switch → health).

**Solution**: Write `test_reject_*` for all security invariants (see Security section).

### 7. Audit Log Format

**Problem**: Unstructured audit logs (e.g., `info!("staged system")`).

**Solution**: Use structured fields for `logd`:

```rust
nexus_log::info!(
    scope: "updated",
    message: "stage ok",
    fields: format!("op=stage\nslot=b\nsize={}\ndigest={}\n", size, hex::encode(digest))
);
```

---

## Quick Reference Commands

### Build & Test

```bash
# Host development
just diag-host          # Check host builds compile
cargo test --workspace  # Run host tests

# OS development
just diag-os           # Check OS services compile for RISC-V
just dep-gate          # CRITICAL: Fail if forbidden crates in OS graph
just test-os           # Run QEMU smoke tests

# Before any commit touching OS code
just dep-gate && just diag-os

# Full test suite
just fmt-check         # Formatting
just lint              # Clippy
just test-all          # Host + OS tests

# Legacy make commands (still supported)
make build             # Full build via Podman
make run               # QEMU run
make test              # Host tests
```

### Debugging

```bash
# Capture serial log
make run 2>&1 | tee build/qemu.log

# Run until specific marker
RUN_UNTIL_MARKER=1 just test-os

# GDB attach (if DEBUG=1)
# QEMU exposes gdb stub on port 1234
```

---

## Success Criteria (from TASK-0007)

### Stop Conditions

**You are DONE when**:

1. ✅ `.nxs` format documented and `nxs-pack` tool implemented
2. ✅ `userspace/updates` library with RAM-based `BootCtrl`
3. ✅ `updated` service registered with `samgrd` and exposing RPCs
4. ✅ Init health gate + rollback logic implemented
5. ✅ bundlemgrd slot-aware publication implemented
6. ✅ All 6 QEMU markers present and green:
   * `updated: ready (non-persistent)`
   * `bundlemgrd: slot a active`
   * `SELFTEST: ota stage ok`
   * `SELFTEST: ota switch ok`
   * `init: health ok (slot <a|b>)`
   * `SELFTEST: ota rollback ok`
7. ✅ Host tests cover stage/switch/health/rollback + negative cases
8. ✅ Security tests (`test_reject_*`) passing
9. ✅ Audit logs to `logd` for all operations
10. ✅ Documentation updated (`docs/updates/`, architecture docs, README)
11. ✅ `just test-all` green
12. ✅ `make test && make build && make run` green

### Acceptance Criteria

* Host tests cover stage/switch/health/rollback and negative cases deterministically.
* QEMU markers prove: `updated` ready, slot switch, health commit, rollback on failure.
* Audit records in `logd` for every stage/switch/health/rollback operation.
* Documentation explains v1.0 model (non-persistent) and path to v1.1 (TASK-0034).

---

## Key Contacts & Resources

### Documentation

* **Agent guidelines**: `docs/agents/PLAYBOOK.md`, `docs/agents/VISION.md`
* **Standards**: `docs/standards/RUST_STANDARDS.md`, `docs/standards/SECURITY_STANDARDS.md`
* **Architecture**: `docs/architecture/README.md` (index)
* **Testing**: `docs/testing/index.md`, `docs/testing/e2e-coverage-matrix.md`

### Code Examples

* **Service pattern**: `source/services/samgrd/`, `source/services/bundlemgrd/`
* **Host-first library**: `userspace/samgr/`, `userspace/bundlemgr/`
* **E2E tests**: `tests/e2e/tests/host_roundtrip.rs`, `tests/vfs_e2e/`
* **QEMU markers**: `source/apps/selftest-client/src/main.rs`

### Related RFCs

* **RFC-0011**: Observability v1 (logd journal + crash reports) - Use for audit logging
* **RFC-0003**: Unified logging - Use `nexus-log` facade

---

## Final Notes

**This is v1.0 (non-persistent)**. The goal is to prove the A/B skeleton works in RAM before adding persistence (TASK-0034). Keep it simple, keep it deterministic, keep it secure.

**Phase 0 is DONE**. Manifest format is unified, drift is resolved. You are starting Phase 1 (System-Set format definition).

**Follow the standards**. Read `PLAYBOOK.md`, `VISION.md`, and `RUST_STANDARDS.md` before writing code. They contain the project's DNA.

**Test everything**. Host tests first (fast, deterministic), then QEMU tests (proof of integration). Write negative tests for all security invariants.

**Ask questions**. If anything is unclear, refer back to the architecture docs or ask for clarification. Better to ask than to implement the wrong thing.

**Good luck!** 🚀

---

## Appendix: File Checklist

### Must-Read Before Starting

- [ ] `docs/agents/PLAYBOOK.md`
- [ ] `docs/agents/VISION.md`
- [ ] `docs/standards/RUST_STANDARDS.md`
- [ ] `docs/standards/SECURITY_STANDARDS.md`
- [ ] `docs/standards/BUILD_STANDARDS.md`
- [ ] `tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md`
- [ ] `docs/adr/0020-manifest-format-capnproto.md`
- [ ] `docs/architecture/README.md`

### Study for Patterns

- [ ] `source/services/samgrd/` (service registration pattern)
- [ ] `source/services/bundlemgrd/` (bundle management pattern)
- [ ] `source/services/execd/` (crash reporting pattern)
- [ ] `userspace/bundlemgr/` (host-first library pattern)
- [ ] `tests/e2e/tests/host_roundtrip.rs` (E2E test pattern)

### Reference During Implementation

- [ ] `docs/architecture/08-service-architecture-onboarding.md`
- [ ] `docs/testing/index.md`
- [ ] `docs/testing/e2e-coverage-matrix.md`
- [ ] `scripts/qemu-test.sh`
- [ ] `tools/nexus-idl/schemas/manifest.capnp`

### Update After Implementation

- [ ] `docs/updates/ab-skeleton-v1.md` (NEW)
- [ ] `docs/architecture/09-nexus-init.md`
- [ ] `docs/architecture/15-bundlemgrd.md`
- [ ] `docs/architecture/07-contracts-map.md`
- [ ] `README.md`
- [ ] `scripts/qemu-test.sh` (add new markers)
