# Supply-Chain v1 Signature Policy

`TASK-0029` adds install-time publisher/key allowlist enforcement with deny-by-default behavior.

## Authority boundaries

- `keystored`: single allowlist authority; loads `recipes/signing/publishers.toml` at startup and answers `IsKeyAllowed`.
- `policyd`: maps allowlist responses into stable policy labels.
- `bundlemgrd`: enforces verify -> policy -> digest -> audit order.

No duplicate allowlist logic is permitted outside `keystored`.

## Allowlist file

Path: `recipes/signing/publishers.toml`

v1 format:

- `version = 1`
- `[[publishers]]` entries with:
  - `id` (publisher identifier as lowercase hex string)
  - `enabled` (boolean)
  - `allowed_algs` (currently open-set text values, e.g. `ed25519`)
  - `keys` (hex-encoded public keys; multiple keys supported for rotation)

Reload model in v1: load-once at `keystored` startup.

## ABI surface

`tools/nexus-idl/schemas/keystored.capnp` exposes:

- `IsKeyAllowedRequest { publisher, alg, pubkey }`
- `IsKeyAllowedResponse { allowed, reason }`

The ABI uses explicit field IDs and reserved gaps to keep v2 expansion non-breaking.

## Failure model

Stable deny labels include:

- `policy.publisher_unknown`
- `policy.key_unknown`
- `policy.alg_unsupported`
- `policy.disabled`
- `integrity.payload_digest_mismatch`
- `audit.unreachable`

Any policy or integrity failure rejects install. Audit emission is synchronous/bounded and fails closed when unreachable.

## Proof commands

```bash
cargo test -p keystored -- is_key_allowed
cargo test -p bundlemgrd -- supply_chain
cargo test -p bundlemgrd test_reject_unknown_publisher
cargo test -p bundlemgrd test_reject_unknown_key
cargo test -p bundlemgrd test_reject_unsupported_alg
cargo test -p bundlemgrd test_reject_payload_digest_mismatch
cargo test -p bundlemgrd test_reject_audit_unreachable
```
