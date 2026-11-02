---
schema: 1
title: "Recovery action tokens (.nxra)"
role: guide
subsystem: security
sources: [userspace/nxra, source/services/bootctld/src/nxra_gate.rs, tools/nx/src/commands/recovery.rs, policies/nxra-trust.toml]
contracts: [RFC-0088, ADR-0055]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Recovery action tokens (.nxra)

> **Purpose** — how an operator signs a break-glass `.nxra` token and where the device enforces it.
> **Owns** — `userspace/nxra/`, `source/services/bootctld/src/nxra_gate.rs`, `tools/nx/src/commands/recovery.rs`
> **Contracts** — [RFC-0088](../rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md) (token format, trust, replay, time model), [ADR-0055](../adr/0055-bootctld-single-boot-state-authority.md) (bootctld is the single boot-state authority)
> **Invariants** — a token only authorizes what standing policy would deny; it never weakens a standing grant. The high-water mark is persisted before the action runs (consume-before-act). A windowed token without a trusted clock rejects `no-clock`.
> **Read next** — [recovery-operations.md](recovery-operations.md)

## Model

- One 136-byte Ed25519-signed token (`nxra::TOKEN_LEN`) authorizes exactly one mutating recovery action.
- Standing capability **or** presented token. Holders of the standing capability (e.g. updated's OTA path) are untouched.
- `userspace/nxra/` is the one parse/verify/sign implementation.
- Trust: a build-time key list, `policies/nxra-trust.toml` → `nxra::BAKED_TRUST`. It is as strong as image integrity. No revocation list: trust changes ship as an image update.
- Replay: a per-(verifier, key) monotone high-water mark, stored as an Integrity envelope at `/state/boot/nxra.hwm.<keyid8>`. A burned token can be re-minted with a higher `seq`.

## Actions

| label | bootctld op |
|---|---|
| `slot-switch` | `OP_SWITCH` (2) |
| `target-set` | `OP_SET_NEXT_BOOT` (7), `OP_SET_TARGET` (8) |
| `reset` | `OP_RESET` (9) |
| `fsck-repair` | defined in the format; statefsd does not check tokens |

## Operator flow

```bash
# sign one action (key = 64 hex chars Ed25519 seed; seq: unix time works)
nx recovery token-make --key operator.hex --action target-set \
    --seq "$(date +%s)" --arg 1 --out token.nxra
# optional window: --not-before <ns> --not-after <ns> (both or neither)

# decode a token and show the verdict against the baked trust
nx recovery token-show token.nxra --json
```

The client appends the 136 token bytes to the mutating bootctld frame at offset 5 (`[B,T,1,op,arg,token…]`).

## Markers

| marker | when |
|---|---|
| `bootctld: nxra accept (key=<id8> action=<label>)` | token verified and consumed |
| `bootctld: nxra reject (reason=<r>)` | `r` ∈ `malformed`, `unknown-version`, `untrusted-key`, `bad-signature`, `action-denied`, `replay`, `expired`, `not-yet-valid`, `no-clock` |
| `SELFTEST: nxra require ok` | standing deny holds without a token |
| `SELFTEST: nxra accept ok` | a token authorizes a switch the machine then rejects as not staged (state-neutral) |
| `SELFTEST: nxra replay deny ok` | the same token dies at the high-water mark |

The proof signer is the deterministic key in `policies/nxra-trust.toml`; its private half is public, so it belongs in proof images only.

## Tests

`cargo test -p nxra`, `cargo test -p nxra_host`, `cargo test -p nx --test recovery_cli`; QEMU: `just test-os headless`.
