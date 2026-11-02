---
schema: 1
title: "Device identity and key custody"
role: guide
subsystem: security
sources: [userspace/identity, source/services/keystored, source/services/rngd, source/services/identityd, userspace/dsoftbus/src/lib.rs, source/services/dsoftbusd/src/os/entry_pure.rs]
contracts: [ADR-0006, RFC-0016, RFC-0008]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Device identity and key custody

> **Purpose** — where the device's Ed25519 identity is made, who holds it on the OS and on the host, and what DSoftBus session authentication actually binds to.
> **Owns** — `source/services/keystored/`, `source/services/rngd/`, `userspace/identity/`, `source/services/identityd/`
> **Contracts** — [ADR-0006](../adr/0006-device-identity-architecture.md) (identity library, IDs from key hashes), [RFC-0016](../rfcs/RFC-0016-device-identity-keys-v1.md) (OS device keys: virtio-rng → rngd → keystored), [RFC-0008](../rfcs/RFC-0008-dsoftbus-noise-xk-v1.md) (Noise XK + identity binding)
> **Invariants** — the private key never leaves keystored (export is always refused). Entropy and key bytes are never logged. Every key operation is policy-gated on the kernel sender id.
> **Read next** — [policyd.md](policyd.md), [signing.md](signing.md), [../network/dsoftbus.md](../network/dsoftbus.md)

## Who holds what

| Piece | Build | Role |
|---|---|---|
| `rngd` | OS | the one entropy authority; drives virtio-rng (`device.mmio.rng`); requests ≤ 256 bytes, caller needs `rng.entropy` |
| `keystored` (`src/os_stub.rs`) | OS | device key custody: keygen from rngd entropy, persist, public-key read, sign; never exports the private key |
| `userspace/identity` | host (std) | Ed25519 `Identity`; `DeviceId` = hex SHA-256 of the verifying key |
| `identityd` | host only | Cap'n Proto `GetDeviceId` / `Sign` / `Verify` over a fresh in-memory `Identity` per start. Excluded from OS builds (`OS_SKIP` in `scripts/discover-services.sh`); nothing on the OS calls it |

## keystored device-key operations (OS)

Wire: `source/services/keystored/src/protocol.rs`. Each op is checked against policyd before any key use.

| Op | Capability | Behavior |
|---|---|---|
| `OP_DEVICE_KEYGEN` (10) | `device.keygen` | 32 bytes from rngd → Ed25519 keypair; seed sealed into an Integrity envelope at `/state/keystore/device.signing` (statefsd, `statefs.keystore`) |
| `OP_GET_DEVICE_PUBKEY` (11) | `device.pubkey.read` | returns the 32-byte public key |
| `OP_DEVICE_SIGN` (12) | `crypto.sign`, or `crypto.derive.statefs` for the exact statefs envelope-derivation label (`statefs::derive::device_sign_allowed`) | Ed25519 signature over ≤ 1 MiB |
| `OP_GET_DEVICE_PRIVKEY` (13) | — | always `STATUS_PRIVATE_EXPORT_DENIED` |
| `OP_DEVICE_RELOAD` (14) | `device.key.reload` | re-reads the sealed seed from statefs (persistence proof) |
| `OP_SIGN` (5) | `crypto.sign` | policy-checked, then `STATUS_UNSUPPORTED`: there is no service signing key; device signing is `OP_DEVICE_SIGN` |
| `OP_VERIFY` (4) | — | Ed25519 verify with a caller-supplied public key (used by `updated`) |

Proof markers: `SELFTEST: device key pubkey ok`, `SELFTEST: device key private export rejected ok`, `SELFTEST: device key persist ok`, `SELFTEST: keystored sign denied ok`.

## DSoftBus session authentication

"Session" here means an authenticated DSoftBus link between two devices. Login sessions belong to `sessiond` and are unrelated.

| Build | Noise XK static key | What the peer proves |
|---|---|---|
| Host (`userspace/dsoftbus`) | derived from the in-process `Identity` (`derive_noise_keys`: SHA-256 of the Ed25519 secret, clamped to X25519) | it signs a proof over its Noise static key and role with its Ed25519 key; the receiver checks the signature and that the `DeviceId` matches discovery |
| OS (`dsoftbusd`) | `derive_test_secret(tag, port)` in `source/services/dsoftbusd/src/os/entry_pure.rs`, labeled `SECURITY: bring-up test keys, NOT production custody` | its Noise static public key equals the one expected for that node; a mismatch holds the daemon (`dsoftbusd: identity mismatch …`), a match prints `dsoftbusd: identity bound peer=node-b` |

On the OS, keystored's device key does not feed the Noise handshake. Binding DSoftBus to the device key is not wired.

Noise XK gives forward secrecy: ephemeral DH results are mixed into the session, so a static key leaked later does not reveal past traffic. Frames above the handshake ride the encrypted transport.

## Rules for changes

- New key use → a keystored op gated by a named capability in `policies/base.toml`, with a `test_reject_*` test.
- Never add a private-key export path, a key in a log line, or a key in an audit record.
- Hardware custody (secure element / TEE) has to stay behind keystored's wire. Callers must not learn where the key lives.
