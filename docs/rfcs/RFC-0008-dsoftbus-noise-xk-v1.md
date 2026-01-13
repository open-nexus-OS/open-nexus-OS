# RFC-0008: DSoftBus Noise XK v1 (no_std handshake + identity binding)

- Status: Complete (Phase 0+1 ✅)
- Owners: @runtime
- Created: 2026-01-07
- Last Updated: 2026-01-10 (Phase 1b proven; host negative tests + QEMU marker harness green)
- Links:
  - Tasks (execution + proof): `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`, `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
  - ADR: `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md` (OS transport contract)
    - `docs/rfcs/RFC-0006-userspace-networking-v1.md` (sockets facade contract)
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (identity-binding rules for security-critical protocols)

## Status at a Glance

- **Phase 0 (Contract seed + host proof surface)**: ✅ **COMPLETE**
-- **Phase 1 (OS/QEMU: no_std handshake + identity binding)**: ✅ **COMPLETE**
  - **Phase 1a: Handshake Implementation**: ✅ **COMPLETE** (`TASK-0003B`)
    - **Real Noise XK handshake** using `nexus-noise-xk` library (X25519 + ChaChaPoly + BLAKE2s)
    - QEMU marker `dsoftbusd: auth ok` emitted **only after successful 3-message handshake**
    - Test keys parameterized (port-based derivation, labeled `SECURITY: bring-up test keys`)
    - Host regression tests green (`cargo test -p nexus-noise-xk`)
    - **Implementation**: `source/services/dsoftbusd/src/main.rs`, `source/apps/selftest-client/src/main.rs`
  - **Phase 1b: Identity Binding Enforcement**: ✅ **COMPLETE** (`TASK-0004`)
    - Host enforces identity mismatch as a hard `AuthError::Identity` reject.
    - OS/QEMU path enforces binding against discovery mapping and keeps marker semantics honest.
- **Phase 2 (Multi-tier Trust + Key Management)**: ⬜ **NOT STARTED**
- **Phase 3 (Enterprise + Ecosystem)**: ⬜ **NOT STARTED**

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The **Noise handshake contract** for DSoftBus session authentication (pattern, cipher suite, framing expectations).
  - The **identity binding rule**: how a peer’s stable identity is bound to the handshake transcript / keys.
  - The **marker semantics** for “auth ok” (it must mean “Noise completed + identity bound”, not “bounded C/R passed”).
- **This RFC does NOT own**:
  - UDP discovery or TCP session establishment (see RFC‑0007).
  - The sockets facade surface or polling/timer model (see RFC‑0006).
  - Kernel crypto, parsers, or distributed routing (future tasks / other RFCs).
  - Long-term key custody (TEE/SE/TPM); this RFC only defines how keys are used, not where they live.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC defines the contract; `TASK-0003B` implements and proves it.

## Context

The current OS DSoftBus milestone uses a bounded deterministic auth gate for bring-up (`dsoftbusd: auth ok`) that is **not** a real Noise handshake. `TASK-0003B` upgrades the OS auth path to a real **Noise XK** handshake while preserving:

- `no_std` + `alloc` constraints on `riscv64imac-unknown-none-elf`,
- bounded resource usage and deterministic proofs (QEMU marker suite),
- userland-only crypto (no kernel participation).

RFC‑0007 already calls out “Noise-based authentication” at a high level; this RFC isolates the security-critical **handshake + identity binding contract** so it can be reviewed and tested without drifting OS transport details.

## Goals

- Define a **clear, versioned** Noise handshake contract for DSoftBus session authentication.
- Ensure **identity binding** is explicit and deterministic (no “accept then warn”).
- Keep the implementation feasible for `no_std` + `alloc` and compatible with the sockets facade.

## Non-Goals

- Multi-VM / cross-device routing semantics.
- A complete provisioning story (keystored/identityd integration is Phase 2 / follow-up tasks).
- A full “remote attestation” protocol (future work).

## Constraints / invariants (hard requirements)

- **no_std + alloc** on OS targets.
- **No fake success**: `dsoftbusd: auth ok` MUST only be emitted after:
  - Noise handshake completes successfully, and
  - identity binding checks pass.
- **Bounded resources**:
  - handshake messages are bounded (max bytes per message),
  - no unbounded buffering or allocation on malformed input.
- **Deterministic proof**:
  - host tests must be deterministic (no external sockets),
  - OS proof uses the canonical QEMU harness and stable markers.
- **Security floor**:
  - discovery is unauthenticated; **authorization boundary is the handshake**.
  - identity is derived from authoritative local sources (or explicit test keys), never solely from peer-supplied strings.
- **Downstream enablement**:
  - handshake logic MUST be parameterizable per node instance (local static + expected remote static + deterministic ephemeral seed),
    so follow-ups (`TASK-0004` dual-node, `TASK-0005` cross-VM, `TASK-0024` QUIC transport) can compose it without rewriting crypto.
  - bring-up mode MUST NOT hardcode a single global keypair; it must be possible to run multiple independent nodes by varying inputs (e.g. per-port derivation).

### Identity binding enforcement (Phase 1b) — normative algorithm

This section defines what “identity bound” means. It is intentionally explicit to avoid drift and “fake green”.

#### Inputs

- **Discovery mapping** for a chosen peer:
  - `peer.device_id` (stable identifier; from discovery packet)
  - `peer.noise_static_pub` (32 bytes; from discovery packet)
  - `peer.addr` + `peer.port` (where to connect)
- **Handshake result**:
  - `handshake.remote_static_pub` (the authenticated static key of the remote peer as determined by Noise XK)

#### Rule (MUST)

After completing the Noise XK handshake, the session implementation MUST check:

- `handshake.remote_static_pub == peer.noise_static_pub`

If (and only if) this holds, the session is “identity bound” to `peer.device_id`.

#### Mismatch handling (MUST)

On mismatch:

- MUST reject/abort the session (no partially-authenticated session state)
- MUST emit `dsoftbusd: identity mismatch peer=<peer.device_id>`
- MUST NOT emit `dsoftbusd: auth ok` for that session

On match:

- MUST emit `dsoftbusd: identity bound peer=<peer.device_id>`
- MAY then emit `dsoftbusd: auth ok` (after the identity-bound marker, or combined into one “auth ok” step as long as semantics are preserved)

#### Required negative tests (host)

Host tests MUST cover identity binding as a hard reject:

- `test_reject_identity_mismatch`: when the announcement claims `device_id=A` but the authenticated static key corresponds to `B`, the connect/accept flow must return `AuthError::Identity(...)` (not “Ok(session) but caller can check”).

Status (2026-01-09):

- OS/QEMU bring-up path implements explicit identity binding checks against the discovery mapping and emits the required markers.
- Host-side hard reject is implemented: identity mismatch is a hard `AuthError::Identity` at the API boundary.
- Host negative tests exist and are green (`cargo test -p dsoftbus -- reject --nocapture`), including:
  - `test_reject_identity_mismatch`
  - `test_reject_malformed_announce`
  - `test_reject_oversized_announce`
  - `test_reject_replay_announce`

#### Required proof markers (OS/QEMU)

The canonical QEMU harness must observe (order may be task-defined, but semantics must hold):

- `dsoftbusd: identity bound peer=<id>`
- `dsoftbusd: auth ok` (meaning identity is bound)

### Implementation notes (quality + “no fake success”)

- **Marker semantics are security-critical**:
  - `dsoftbusd: auth ok` MUST NOT mean “handshake bytes exchanged”; it MUST mean “Noise completed AND identity bound”.
- **Binding is not implied**:
  - A successful handshake alone is not sufficient if the expected remote static key was not sourced from discovery mapping. The code MUST make the mapping explicit and check it.
- **Mismatch is a hard failure**:
  - Mismatch MUST become `AuthError::Identity` (or OS equivalent) and MUST terminate the session deterministically.
- **Do not log secrets**:
  - Do not print keys, nonces, or transcript hashes in logs/markers.

#### QEMU bring-up note (discovery source constraints)

In Phase 1 OS/QEMU bring-up, the discovery “mapping” used for identity binding may originate from a deterministic UDP loopback path (see RFC‑0007).
This does **not** weaken the Phase 1b invariant: identity binding is still mandatory and the mismatch behavior is still a hard reject.
However, it means Phase 1b proofs validate **binding correctness**, not “subnet discovery realism”; subnet discovery is covered by follow-on phases/tasks.

## Proposed design

### Contract / interface (normative)

#### Noise pattern + suite

The DSoftBus session authentication handshake SHALL implement:

- Noise pattern: **XK**
- Protocol name: **`Noise_XK_25519_ChaChaPoly_BLAKE2s`**

Notes:

- The crypto suite choice is intentionally the standard Noise tuple above to avoid “custom crypto”.
- If the chosen no_std implementation cannot provide this exact suite, any deviation MUST be:
  - versioned (new protocol name),
  - documented here,
  - and proven with deterministic tests + marker gates before switching defaults.

#### Roles and keys

- **Initiator**: the connecting side (client).
- **Responder**: the listening side (server).
- Each side has a **static X25519 keypair**:
  - `s_i` / `S_i` (initiator private/public)
  - `s_r` / `S_r` (responder private/public)

Key sourcing (Phase 0/1):

- For bring-up, static keys MAY be **test keys** (compile-time or boot-time injected), but MUST be explicit in code/docs and MUST NOT be described as "secure storage".
- Deterministic ephemeral keys/seeds MAY be used for reproducible proofs, but MUST be explicitly labeled **test-only / non-secure** and MUST NOT be claimed as production entropy.
- Phase 2 integrates with `keystored`/`identityd` with multi-tier trust model (see below).

Key sourcing (Phase 2 - Multi-tier Trust Model):

**Design Principle**: Support multiple trust models based on use-case, simple by default.

#### Tier 1: PIN-based TOFU (Consumer Default)

**Use-case**: Consumer devices, home network, peer discovery

**Mechanism**:

- Trust-On-First-Use with numeric PIN verification (like Bluetooth Simple Secure Pairing)
- User consent required for first contact
- Zero external dependencies (no PKI, no certificates)

**Flow**:

1. Discovery: Device A finds Device B
2. Noise handshake starts (exchange ephemeral keys)
3. Compute: `PIN = HMAC-BLAKE2s(handshake_hash, "Neuron-PIN-v1")[0..6] % 1000000`
4. UI shows PIN on both devices: "Device B wants to connect. PIN: 123456"
5. User verifies PINs match on both screens
6. If confirmed: complete handshake, store `(device_id <-> noise_static_pub)` with `user_approved=true`
7. Future sessions: auto-accept if key matches stored entry

**Storage**: Local registry only (`state:/dsoftbus/peers.json`)

**Security Properties**:

- Prevents MITM during first contact (active attacker must display same PIN)
- 6-digit PIN = 1-in-1,000,000 chance of collision
- User-friendly (no technical knowledge required)
- Privacy-preserving (no third party involvement)

#### Tier 2: PKI (Enterprise Opt-in)

**Use-case**: Corporate devices, IoT fleets, compliance requirements

**Mechanism**:

- X.509 certificates signed by corporate CA
- Certificate chain validation during handshake
- OCSP/CRL for revocation checking
- Noise static key derived from certificate private key

**Flow**:

1. Device provisioned with certificate (signed by Corp CA)
2. Discovery includes certificate fingerprint
3. Noise handshake: peer sends certificate chain
4. Verify: chain validation, expiry, revocation status
5. Extract: Noise static public key from certificate
6. Bind: `device_id` to certificate subject (CN or SAN)
7. Store: certificate + validation timestamp

**Storage**: Certificate store + CRL cache

**Integration with keystored**:

```rust
keystored.deriveNoiseKeyFromCert(cert_id) -> static_secret
identityd.validateCertChain(cert, ca_bundle) -> Result<ValidCert, RevocationReason>
```

**Security Properties**:

- Cryptographic chain of trust
- Centralized revocation (OCSP/CRL)
- Compliance-ready (audit logs, key escrow)
- Recovery: revoke + reissue

#### Tier 3: Web-of-Trust (Future - Phase 3)

**Use-case**: Decentralized networks, community mesh, no central authority

**Mechanism**:

- Transitive trust (friend-of-friend)
- Trust scores based on endorsements
- Reputation tracking

**Status**: Phase 3 (not yet defined)

#### Key Rotation Policy (all tiers)

Static keys MUST support rotation without breaking existing sessions:

**Mechanism**:

- Key versioning: Each device has `key_id` (timestamp or monotonic counter)
- Grace period: Old keys remain valid for N days during rotation (default: 30 days)
- Discovery: Include `key_id` in announcement
- Session: Accept if peer's `key_id` is not explicitly revoked

**Integration**:

```rust
keystored.rotateNoiseKey(reason: RotationReason) -> new_static_secret
identityd.revokeKey(key_id, reason: RevocationReason) -> revocation_timestamp
```

**Rotation triggers**:

- Periodic: Every 90 days (configurable)
- Compromise: Immediate revocation + emergency rotation
- Policy: Admin-initiated rotation

**Backward compatibility**:

- Sessions established with old key continue (session keys remain valid)
- New sessions require new key (after grace period)
- Discovery announces both keys during grace period

Bring-up reference implementation contract (current repo):

- OS/QEMU proof uses deterministic test keys derived from the session port:
  - `server_static_secret = derive_test_secret(tag=0xA0, port)`
  - `client_static_secret = derive_test_secret(tag=0xB0, port)`
  - `server_eph_seed    = derive_test_secret(tag=0xC0, port)`
  - `client_eph_seed    = derive_test_secret(tag=0xD0, port)`
- Where `derive_test_secret(tag, port)` is a deterministic 32-byte derivation (NOT secure) intended only for proof reproducibility and multi-node bring-up.

#### Identity binding (normative rule)

After a successful Noise handshake, the authenticated session MUST be bound to a stable **peer identity** as follows:

- The peer's claimed identity token (e.g. `device_id`) is **non-authoritative** until verified.
- The implementation MUST verify that the peer's stable identity is consistent with:
  - the peer's static public key observed/validated by the Noise handshake, and
  - the local policy for mapping identity → expected static key (or expected fingerprint).

**Binding rules vary by trust tier**:

#### Phase 1 (Bring-up - Test Keys)

Minimum deterministic rule for OS/QEMU bring-up:

- If discovery advertises `(device_id, noise_static_pub)` then the session is accepted only if:
  - the Noise handshake authenticates the peer as `noise_static_pub`, and
  - `device_id` matches the locally expected identity for that `noise_static_pub` (or vice versa), per a deterministic mapping table.

This keeps identity binding explicit without requiring a full attestation stack.

#### Phase 2 (Production - Multi-tier Trust)

##### Tier 1: PIN-based TOFU

- First contact: User confirms PIN match → store `(device_id <-> noise_static_pub)` binding
- Subsequent: Accept if handshake authenticates stored `noise_static_pub`
- Reject: If `device_id` unchanged but `noise_static_pub` differs (key mismatch attack)
- UI prompt: "Device X's key changed. This could indicate an attack. Accept new key?"

##### Tier 2: PKI

- Discovery: peer advertises `device_id` + certificate fingerprint
- Handshake: peer sends certificate chain
- Validate: certificate chain, expiry, revocation
- Bind: `device_id` MUST match certificate subject (CN or SAN)
- Reject: If mismatch or invalid chain

**Deterministic mapping enforcement**:

- When discovery selects peers (subnet / cross-VM), `device_id` bytes MUST remain non-authoritative until bound to the authenticated `noise_static_pub`.
- The system MUST define and enforce a deterministic mapping `(device_id <-> noise_static_pub)` (initially via local registry; Tier 2 via PKI).

#### Framing and bounds

- Noise handshake messages MUST be carried inside the DSoftBus session establishment over TCP (or any session stream) as **bounded, length-delimited frames**.
- A receiver MUST reject:
  - frames exceeding the negotiated/declared maximum handshake size,
  - truncated frames,
  - invalid Noise messages,
  with deterministic errors and without panics.

### Phases / milestones (contract-level)

- **Phase 0 (host proof surface)**: ✅ **COMPLETE**
  - Add deterministic tests proving:
    - handshake happy path,
    - handshake failure (wrong static key / identity mismatch),
    - bounded frame rejection.
  - **Proof**: `cargo test -p nexus-noise-xk -- --nocapture` (3 tests)
  
- **Phase 1a (OS/QEMU: Handshake Implementation)**: ✅ **COMPLETE** (`TASK-0003B`)
  - Wire the Noise XK handshake into `dsoftbusd` OS auth path.
  - QEMU marker `dsoftbusd: auth ok` emitted after successful handshake.
  - Test keys parameterized (port-based derivation for dual-node scenarios).
  - **Proof**: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (green)
  - **What's done**: Noise handshake cryptography + QEMU proof
  - **What's pending**: Identity binding enforcement (Phase 1b)
  
- **Phase 1b (OS/QEMU: Identity Binding Enforcement)**: ⬜ **PENDING** → **TASK-0004**
  - Implement identity binding rule: `(device_id <-> noise_static_pub)` mapping
  - Verify peer's authenticated `noise_static_pub` matches expected key for `device_id`
  - Reject session if mismatch (no `auth ok` marker for mismatched sessions)
  - **Why deferred to TASK-0004**: Requires dual-node mode to be meaningfully tested (single-node is trivial self-verification)
  - **Marker**: `dsoftbusd: identity bound peer=<id>`
  - **Proof**: QEMU markers + identity mismatch rejection test
- **Phase 2 (Multi-tier Trust + Key Management)**: ⬜ **NOT STARTED**
  - **Goals**:
    - Tier 1 (Consumer): PIN-based TOFU with user consent
    - Tier 2 (Enterprise): PKI with X.509 certificates + OCSP
    - Key rotation policy with grace periods
    - `keystored`/`identityd` integration
  - **Tracking**: To be created (Phase 2 task)
  - **Unblocks**: TASK-0024 (QUIC transport can proceed after Tier 1 is complete)
- **Phase 3 (Enterprise + Ecosystem)**: ⬜ **NOT STARTED**
  - **Goals**:
    - DTLS hybrid option (for PKI validation + Noise efficiency)
    - Web-of-Trust (Tier 3, decentralized)
    - Cross-ecosystem interop (OpenHarmony adapter)
  - **Tracking**: Future tasks

## Security considerations

- **Threat model**:
  - spoofed discovery announcements,
  - replay of handshake messages,
  - identity confusion between `device_id` strings and authenticated keys,
  - resource exhaustion (oversized frames / busy loops).
- **Mitigations**:
  - Noise handshake authenticates keys and encrypts session traffic,
  - deterministic identity binding rule prevents “string spoofing”,
  - bounded frames + deterministic errors prevent allocator/loop abuse.
- **Open risks**:
  - bring-up keys are not secure custody; Phase 2 must tighten provisioning and policy.

## Failure model (normative)

- Handshake failures MUST:
  - fail closed (no authenticated session created),
  - avoid emitting `auth ok`,
  - return deterministic errors to callers.
- Malformed inputs MUST NOT panic.

## Proof / validation strategy (required)

Tasks are execution truth; see `tasks/TASK-0003B-dsoftbus-noise-xk-os.md` for canonical proof commands.

### Proof (Host)

- `cd /home/jenning/open-nexus-OS && cargo test -p nexus-noise-xk -- --nocapture`
- (optional higher-layer regression) `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- --nocapture`

### Proof (OS/QEMU)

- `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

### Deterministic markers (required meaning)

- `dsoftbusd: auth ok` → **Noise XK handshake success + identity binding success**

## Alternatives considered

- Keep bounded challenge/response gate: rejected (not Noise; too easy to over-trust; not a real AEAD session).
- Kernel crypto: rejected (TCB growth; violates “crypto in userland services” direction).

## Open questions

- Which no_std Noise implementation is acceptable for our constraints (dependency size, auditability, features)?
- Exact key provisioning interface between `dsoftbusd` and `keystored`/`identityd` (Phase 2).

## Checklist (keep current)

- [x] Scope boundaries are explicit; cross-RFC ownership is linked (RFC‑0006/0007/0005).
- [x] Task exists for the milestone and contains stop conditions + proof (`TASK-0003B`).
- [x] Proof is "honest green" (markers/tests), not log-grep optimism. (QEMU marker gate is green in `TASK-0003B`.)
- [x] Determinism + bounded resources are specified and regression-tested. (Host regression tests exist for `nexus-noise-xk`.)
- [x] Security invariants are stated and have at least one regression proof (host tests + QEMU marker gate).
- [ ] If claiming stability: on-wire/handshake framing vectors + compat tests exist.
  - **Status (2026-01-07)**: Intentionally unchecked — no stability claim yet (see note below)
- [x] **Stubs Labeling**: Stubs (if any) are explicitly labeled and non-authoritative.
  - **Status (2026-01-07)**: ✅ **FULFILLED** — test keys explicitly labeled in code
  - **Implementation**: All `derive_test_secret` calls in `dsoftbusd` and `selftest-client` have:

    ```rust
    // SECURITY: bring-up test keys, NOT production custody
    // These keys are deterministic and derived from port for reproducibility only.
    ```

### Gap Summary (as of 2026-01-07)

**Phase 1a (Noise XK Handshake) is 100% complete:**

- ✅ Real Noise XK handshake implemented in `dsoftbusd` + `selftest-client`
- ✅ Uses `nexus-noise-xk` library (X25519 + ChaChaPoly + BLAKE2s)
- ✅ Test keys explicitly labeled with `SECURITY: bring-up test keys`
- ✅ QEMU marker `dsoftbusd: auth ok` emitted only after successful 3-message handshake

**For Phase 1b (Identity Binding) — deferred to TASK-0004:**

1. **GAP 3** (from RFC-0007): Identity binding enforcement
   - Handshake works, but no code verifies `device_id <-> noise_static_pub` mapping
   - Requires **dual-node mode** to be meaningfully tested (single-node is trivial self-verification)
   - Tracked in: TASK-0004
   - Blocks: TASK-0005, TASK-0024

**Stability note:**

- This RFC does **not** currently claim a stable on-wire framing format for the handshake beyond "bounded frames".
  If/when we introduce a stable framing (e.g. for QUIC transport or cross-VM interoperability), we must add versioned vectors + compat tests and then check the stability box above.
