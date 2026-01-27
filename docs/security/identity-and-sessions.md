# Identity and session security

Open Nexus OS assigns every device a long-term Ed25519 identity. The identity
material is treated as high-value secret state and is intended to be held by a
dedicated authority service rather than being copied into arbitrary callers.

This follows the system hybrid approach: keep APIs stable now, and later move key custody to secure
hardware (Secure Element / TEE) per device class without ABI churn (see `docs/agents/VISION.md`).

## Device identity keys on OS builds (virtio-rng → rngd → keystored)

On OS/QEMU builds, “real” device identity keys require real entropy. OS builds
must not depend on `getrandom`, so entropy is provided via:

- a userspace **virtio-rng** frontend (MMIO),
- `rngd` as the **single entropy authority** (bounded requests),
- `keystored` as the **device key custody** authority:
  - generates the device identity keypair using entropy from `rngd`,
  - exposes **only** the public key (no private key export),
  - performs signing operations without releasing private key material.

All sensitive operations are deny-by-default via `policyd`, binding to
kernel-provided `sender_service_id`, and allow/deny decisions are audit-logged
via `logd`.

Contract/proofs:

- Task: `tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md`
- RFC: `docs/rfcs/RFC-0016-device-identity-keys-v1.md`
- MMIO primitive: `tasks/TASK-0010-device-mmio-access-model.md`
- Persistence/rotation is out of scope here (see `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` and follow-ups).

## Identity service surface (today)

The identity daemon (`identityd`) is intended to be the single entry point for
other userland services that need identity operations. In early OS bring-up,
key custody and signing are enforced in `keystored`, and identity APIs may
either call into `keystored` or be consolidated as the system hardens.

If/when `identityd` is the public-facing API surface, it should expose three
Cap'n Proto calls:

- `GetDeviceId` returns the textual identifier derived from the verifying key.
- `Sign` produces signatures for attestation payloads.
- `Verify` checks signatures against provided verifying keys.

DSoftBus-lite consumes these APIs during session establishment. Each node derives
a Noise static keypair from the Ed25519 secret and signs a proof containing the
Noise public key plus a role tag (client or server). During the Noise XK
handshake, both peers exchange proofs and verify them via `identityd`. A
successful handshake yields an AEAD-protected transport with forward secrecy:

1. Long-term Ed25519 keys authenticate the Noise static keys.
2. Noise XK mixes ephemeral DH results into the session, so compromising a static
   key after the fact does not reveal past traffic.
3. Cap'n Proto control frames ride on top of the encrypted transport, keeping
   service routing metadata confidential.

Future work will store identity material in a secure element, integrate mutual
attestation for higher-level services, and extend the session lifecycle with
rekeying support. The current implementation establishes the primitives needed
for host-first distributed testing while keeping the kernel free from key
management responsibilities.
