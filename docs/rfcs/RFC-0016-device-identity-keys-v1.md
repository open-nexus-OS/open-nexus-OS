# RFC-0016: Device Identity Keys v1 — virtio-rng + rngd authority + keystored keygen

- Status: Done
- Owners: @runtime @security
- Created: 2026-01-26
- Last Updated: 2026-01-26
- Links:
  - Tasks: `tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md`
  - ADRs: `docs/adr/0006-device-identity-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy + audit baseline)
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (capability model)

## Status at a Glance

- **Phase 0 (virtio-rng frontend)**: ✅ Implemented (host tests + OS compile check)
- **Phase 1 (rngd authority service)**: 🟨 Partially implemented (IPC + policy gate done; virtio-rng MMIO integration pending)
- **Phase 2 (keystored keygen + pubkey API)**: 🟨 Partially implemented (protocol + OS-lite logic done; end-to-end QEMU proof pending)
- **Phase 3 (QEMU smoke + negative proofs)**: 🟨 Partially implemented (host negative tests added; QEMU markers/harness integration pending)

Definition: "Complete" means the contract is defined and the proof gates are green.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in the task.

- **This RFC owns**:
  - virtio-rng frontend API contract (bounded read primitive)
  - rngd IPC protocol (wire format, ops, error codes)
  - keystored device keygen API (entropy request, pubkey export, signing delegation)
  - Policy capability names for 8B gates
  - Security invariants for entropy + keygen paths

- **This RFC does NOT own**:
  - DRBG design, entropy mixing, reseeding policies (out of scope; minimal + auditable)
  - Key persistence/rotation (TASK-0009/TASK-0159/TASK-0160)
  - Production hardware entropy sources (QEMU bring-up only)
  - Kernel MMIO primitives (owned by TASK-0010)

### Relationship to tasks

- `tasks/TASK-0008B-*` defines stop conditions and proof commands.
- This RFC defines stable contracts that the task implements.

## Context

TASK-0008 established policy authority + audit semantics. Secure device identity keys require
**real entropy** on OS builds. Host builds use `OsRng`; OS builds cannot use `getrandom`.

On QEMU `virt`, virtio-rng is available as an MMIO device (address `0x10007000`, IRQ 8).
With TASK-0010's MMIO mapping primitive, we can implement a userspace virtio-rng frontend
and expose an entropy service (`rngd`) to keystored and other consumers.

## Goals

1. Provide bounded entropy reads from virtio-rng hardware via a userspace service (`rngd`).
2. Establish `rngd` as the **single entropy authority** (other services MUST NOT access virtio-rng directly).
3. Enable `keystored` to generate a device identity keypair (Ed25519) using entropy from `rngd`.
4. Expose **only** the device public key; forbid any private key export.
5. Gate all sensitive operations via `policyd` and audit via `logd`.

## Non-Goals

- Production-grade hardware entropy (QEMU bring-up only).
- Full DRBG, entropy pool mixing, or reseeding (keep minimal + auditable).
- Device key persistence or rotation (deferred to TASK-0009/TASK-0159).
- Kernel changes beyond TASK-0010's existing MMIO primitives.

## Constraints / invariants (hard requirements)

- **Determinism**: Markers and proofs are deterministic; no timing-fluke behavior.
- **No fake success**: Never emit "ok/ready" markers unless real behavior occurred.
- **Bounded resources**: All entropy requests bounded (max 256 bytes per request).
- **Security floor**:
  - Entropy bytes MUST NOT be logged.
  - Device private keys MUST NEVER be returned via any API.
  - All operations MUST be authorized via `sender_service_id` (channel identity).
  - All allow/deny decisions MUST be audit-logged via logd.
- **Stubs policy**: Any stub must be labeled and MUST NOT claim success.
- **OS dep hygiene**: MUST NOT add `getrandom` or `parking_lot` to OS graphs.

## Proposed design

### Contract / interface (normative)

#### 1. virtio-rng Frontend Library (`source/drivers/rng/virtio-rng/`)

**API:**

```rust
/// Bounded entropy read from virtio-rng MMIO device.
/// Returns `Err(RngError::Oversized)` if `n > MAX_ENTROPY_BYTES`.
/// Returns `Err(RngError::Timeout)` if device does not respond within bound.
pub fn read_entropy(mmio_base: *mut u8, n: usize) -> Result<Vec<u8>, RngError>;

const MAX_ENTROPY_BYTES: usize = 256;
```

**Invariants:**
- MMIO mapping is capability-gated (via `DeviceMmio` cap from init).
- Mapping is USER|RW, never executable (W^X).
- Polling-based (no IRQ handling in v1).

#### 2. rngd IPC Protocol (`source/services/rngd/`)

**Wire format (versioned byte frames):**

```text
Request:  [MAGIC0='R', MAGIC1='G', VERSION=1, OP, nonce:u32le, payload...]
Response: [MAGIC0='R', MAGIC1='G', VERSION=1, OP|0x80, STATUS, nonce:u32le, payload...]
```

**Operations:**

| Op | Name | Request | Response |
|----|------|---------|----------|
| 1 | GET_ENTROPY | `[nonce:u32le, n:u16le]` | `STATUS, nonce:u32le, entropy_bytes...` |

**Status codes:**

| Code | Name | Meaning |
|------|------|---------|
| 0 | OK | Success |
| 1 | OVERSIZED | Request exceeds MAX_ENTROPY_BYTES |
| 2 | DENIED | Policy check failed |
| 3 | UNAVAILABLE | Device not ready |

**Policy gate:** Caller must have `rng.entropy` capability (checked via `policyd` OP_CHECK_CAP).

**Reply routing (robustness):**
- Callers that do not have a dedicated per-client response endpoint MUST use `CAP_MOVE` with `@reply` and correlate the response via `nonce`.
- rngd MUST echo the nonce unchanged so callers can ignore unrelated replies on shared inboxes.

**Delegated policy checks (bring-up contract):**
- `rngd` and `keystored` are enforcement points that must authorize actions based on the *requesting service id*.
- Policyd v1 `OP_CHECK_CAP` binds the subject to the policyd caller and cannot check third-party subjects.
- Therefore, v1 introduces `OP_CHECK_CAP_DELEGATED`:
  - Policyd verifies the enforcement point has `policy.delegate`
  - Policyd then evaluates `(subject_id, cap)` and audits the decision
  - Responses are sent on the caller’s dedicated policyd response endpoint (no CAP_MOVE)

#### 3. keystored Device Keygen API Extensions

**New operations (added to existing keystored wire protocol):**

| Op | Name | Request | Response |
|----|------|---------|----------|
| 10 | DEVICE_KEYGEN | `[]` | `STATUS` |
| 11 | GET_DEVICE_PUBKEY | `[]` | `STATUS, pubkey[32]` |
| 12 | DEVICE_SIGN | `[payload_len:u32le, payload...]` | `STATUS, signature[64]` |

**Policy gates:**
- `DEVICE_KEYGEN` requires `device.keygen` capability
- `GET_DEVICE_PUBKEY` requires `device.pubkey.read` capability (or allow for all services if less restrictive)
- `DEVICE_SIGN` requires `crypto.sign` capability (existing)

**Status codes (new):**

| Code | Name | Meaning |
|------|------|---------|
| 10 | KEY_EXISTS | Device key already generated |
| 11 | KEY_NOT_FOUND | Device key not yet generated |
| 12 | PRIVATE_EXPORT_DENIED | Attempt to export private key |

**Invariant:** `keystored` MUST never expose private key material via any API path.

### Phases / milestones (contract-level)

- **Phase 0 (virtio-rng frontend)**: Library compiles for host+OS; mock for host tests; MMIO read for OS.
- **Phase 1 (rngd service)**: IPC loop, policy gate, `rngd: ready` marker, `SELFTEST: rng entropy ok`.
- **Phase 2 (keystored keygen)**: Device keygen via rngd, pubkey API, `SELFTEST: device key pubkey ok`.
- **Phase 3 (negative proofs)**: `test_reject_*` tests + `SELFTEST: device key private export rejected ok`.

## Security considerations

### Threat model

- **Entropy spoofing**: Caller or compromised service fakes RNG output.
- **Key extraction**: Private key leaks via API, logs, or memory dumps.
- **Unauthorized keygen**: Unprivileged service triggers device key generation.
- **DoS via unbounded reads**: Unbounded entropy requests starve the system.

### Security invariants (MUST hold)

- Entropy bytes MUST NOT be logged (including in error messages).
- Device private keys MUST NEVER be returned (signing happens inside keystored).
- Calls MUST be authorized based on `sender_service_id` (channel identity, not payload).
- All allow/deny decisions for entropy/keygen MUST be audit-logged via logd.

### DON'T DO list

- DON'T log entropy bytes, seeds, or private key material.
- DON'T allow any private key export path (even "debug" or "test" variants).
- DON'T skip policy checks for "trusted" services.
- DON'T accept unbounded entropy requests.
- DON'T add `getrandom` to OS dependency graph.

### Mitigations

- Policy-gated via `policyd` OP_CHECK_CAP for all sensitive operations.
- Bounded entropy requests (max 256 bytes).
- `sender_service_id` binding for all policy decisions.
- Audit trail via logd for all allow/deny decisions.

## Failure model (normative)

| Condition | Behavior | Status |
|-----------|----------|--------|
| Entropy request > 256 bytes | Reject with OVERSIZED | Deterministic |
| Caller lacks `rng.entropy` | Reject with DENIED | Deterministic |
| Device not available | Reject with UNAVAILABLE | Deterministic |
| Private key export attempt | Reject with PRIVATE_EXPORT_DENIED | Deterministic |
| Keygen called when key exists | Reject with KEY_EXISTS | Deterministic |

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p keystored -- reject --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p rngd -- reject --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p rng-virtio -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
cd /home/jenning/open-nexus-OS && RUN_PHASE=policy RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `rngd: ready` — rngd service is ready to serve entropy requests
- `SELFTEST: rng entropy ok` — Bounded entropy request succeeded
- `SELFTEST: device key pubkey ok` — Device pubkey export succeeded
- `SELFTEST: device key private export rejected ok` — Private export correctly rejected

### Boot integration (required for QEMU proof)

- `rngd` MUST be included in the init-lite service image list and spawned before `init: ready`.
- init-lite routing MUST provide `route_get("rngd")` to selftest-client and keystored.

### Policy capabilities (add to `recipes/policy/base.toml`)

```toml
[rngd]
caps = ["device.mmio.rng"]

[keystored]
caps = ["rng.entropy", "device.keygen"]

[selftest-client]
caps = ["rng.entropy", "device.pubkey.read"]
```

## Alternatives considered

1. **Direct virtio-rng access from keystored**: Rejected. Violates "single authority" principle; harder to audit.
2. **Kernel-level entropy service**: Rejected. Goes against "kernel minimal" vision.
3. **Deterministic test keys always**: Rejected for 8B. That's bring-up only; real entropy is the goal.

## Open questions

- Q1: Should `GET_DEVICE_PUBKEY` be policy-gated or open to all services? **Decision**: Start gated (`device.pubkey.read`), can relax later.
- Q2: virtio-rng MMIO address hardcoded for QEMU virt or discovered? **Decision**: Hardcoded for v1 (0x10007000).

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: virtio-rng frontend library — proof: `cargo test -p rng-virtio`
- [ ] **Phase 1**: rngd service + IPC + policy — proof: `RUN_PHASE=bring-up just test-os` shows `rngd: ready`
  - Note: OS-lite rngd currently does not use virtio-rng MMIO; virtqueue/MMIO wiring is required to meet “real entropy” goal.
- [ ] **Phase 2**: keystored keygen + pubkey API — proof: `SELFTEST: device key pubkey ok`
  - Note: keystored OS-lite now implements keygen/pubkey/sign ops, but QEMU proof requires rngd to be started by init-lite and the harness markers to be enforced.
- [ ] **Phase 3**: negative proofs — proof: `cargo test -- reject && SELFTEST: device key private export rejected ok`
  - Host-only negative tests exist for rngd request bounds + deny-by-policy.
- [x] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`) (host-side for rngd).
