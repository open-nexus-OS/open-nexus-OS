# Identity: `identityd` + keystore direction — onboarding

Open Nexus OS assigns each device a long-term identity and uses it to authenticate sessions and sign/verify payloads.

Canonical sources:

- Identity and session security: `docs/security/identity-and-sessions.md`
- Device identity decision: `docs/adr/0006-device-identity-architecture.md`
- Policy authority + audit: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
- Service architecture context: `docs/adr/0017-service-architecture.md`
- Testing guide: `docs/testing/index.md`

## Responsibilities

### `userspace/identity` (domain library)

- Generates and manages Ed25519 identity material (host-first).
- Derives stable device IDs from the verifying key.
- Implements signing and verification helpers.

### `identityd` (service daemon)

`identityd` is the **single entry point** for other services that need identity operations.
Per `docs/security/identity-and-sessions.md`, it exposes calls like:

- `GetDeviceId`
- `Sign`
- `Verify`

This keeps key handling centralized and auditable.

## Relationship to DSoftBus / sessions

Identity underpins secure distributed sessions:

- DSoftBus-lite uses identity to bind Noise keys to device identities.
- Session establishment can prove possession of identity keys without exposing secrets.

See `docs/security/identity-and-sessions.md` for the full handshake narrative.

## Keystore direction (why this is "hybrid root")

Today, keys may be host/QEMU-friendly and stored in memory or simple persistence, but the design goal is:

- keep the API surface stable now,
- move key custody later into secure hardware (Secure Element / TEE) **without ABI churn**.

That's why identity operations are mediated by `identityd` and why keystore integration is treated as an authority boundary.

## Policy-Gated Signing (TASK-0008)

As of TASK-0008, `keystored` enforces policy-gated signing:

- `OP_SIGN` requires the `crypto.sign` capability in `recipes/policy/base.toml`
- Policy check binds to `sender_service_id` (kernel-provided, unforgeable)
- Denials are audit-logged and return `STATUS_DENIED_BY_POLICY`
- Private keys never leave keystored (signatures are returned, not key material)

**QEMU proof marker:** `SELFTEST: keystored sign denied ok`

## Device identity keys on OS builds (TASK-0008B)

OS builds cannot depend on `getrandom`, so “real” device identity keys require an OS-native entropy path.
The v1 direction is:

- **Entropy source**: userspace virtio-rng frontend (MMIO on QEMU `virt`)
- **Entropy authority**: `rngd` is the single entropy authority (bounded requests; policy-gated; audited)
- **Key custody**: `keystored` generates the device identity keypair using `rngd` entropy and forbids any private key export
  (public key export is allowed only via explicit policy gate).

Security invariants:

- No entropy bytes or private key material is logged (UART/logd).
- Authorization binds to `sender_service_id` and is deny-by-default via `policyd`.
- Allow/deny decisions are audit-logged via `logd`.

Contract + proofs:

- Task: `tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md`
- RFC: `docs/rfcs/RFC-0016-device-identity-keys-v1.md`

Key persistence/rotation is explicitly out of scope for 8B and is handled by persistence/lifecycle follow-ups
(see `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` and the identity/keystore lifecycle tasks).

## Proof expectations

- Host-first tests should cover key derivation and signing/verification behavior deterministically.
- E2E harnesses should validate session establishment flows without requiring QEMU until OS transport work is real.
