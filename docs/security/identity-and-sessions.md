# Identity and session security

Open Nexus OS assigns every device a long-term Ed25519 identity. The
`userspace/identity` crate generates the keypair, derives a stable textual device
identifier by hashing the verifying key, and exposes helpers to sign or verify
payloads. Keys are currently kept in-memory, but the API is designed so that a
future keystore can back the same serialization logic without touching callers.

The identity daemon (`identityd`) is the single entry point for other userland
services. It exposes three Cap'n Proto calls:

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
