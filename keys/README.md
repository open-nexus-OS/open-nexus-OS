<!--
SPDX-License-Identifier: Apache-2.0

CONTEXT: Public key material for the `nexus-evidence` Phase-5
sealing pipeline. Only PUBLIC keys live here; private keys NEVER
land in this tree (or anywhere in the repo) — see
`.gitignore` and `tools/gen-ci-key.sh` (P5-04).

OWNERS: @runtime
STATUS: Functional (P5-03 placeholder; rotated in P5-04)
API_STABILITY: Public-key bytes are stable across the lifetime of
each key; rotation requires an RFC-tracked migration window.
-->

# Evidence Bundle Public Keys

This directory holds the **public** key material that
`tools/verify-evidence.sh` and `nexus-evidence verify` consume.

## Layout

| File                            | Class    | Source                                                 |
| ------------------------------- | -------- | ------------------------------------------------------ |
| `evidence-ci.pub.ed25519`       | CI       | Pinned in CI; private key in CI secret store           |

The bringup public key is **not** stored in the repo: each
developer keeps their own under
`~/.config/nexus/bringup-key/public.ed25519` (P5-04).

## Encoding

Each `*.pub.ed25519` file contains **64 hex characters + trailing
newline** = the raw 32-byte Ed25519 public key. This is what
`nexus-evidence keygen --pubkey-out=…` writes (P5-03) and what
`tools/gen-ci-key.sh` will write in P5-04.

## P5-03 Placeholder (current)

`evidence-ci.pub.ed25519` was generated at P5-03 from a deterministic
seed for bring-up purposes. The matching private key is **not**
treated as confidential and **must not** be used for any real
sealing.

## Rotating the CI key (P5-04)

```sh
# 1. Build the CLI once (the script picks up target/{debug,release}).
cargo build -p nexus-evidence

# 2. Remove the placeholder pubkey so the script will write a fresh one.
rm keys/evidence-ci.pub.ed25519

# 3. Generate a new keypair. The script writes the public key to
#    keys/evidence-ci.pub.ed25519 (chmod 0644) and prints the
#    base64-encoded private seed on stdout. The private seed never
#    touches the filesystem.
tools/gen-ci-key.sh

# 4. Paste the printed base64 into the CI secret named
#    NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64, then close the terminal.

# 5. Commit `keys/evidence-ci.pub.ed25519` together with the RFC tick
#    that documents the rotation window.
```

## Rotating a bringup key (per developer)

```sh
cargo build -p nexus-evidence
tools/gen-bringup-key.sh
# Writes:
#   ~/.config/nexus/bringup-key/private.ed25519 (chmod 0600)
#   ~/.config/nexus/bringup-key/public.ed25519  (chmod 0644)
```

## DO NOT

- Commit any file matching `*.private*`, `*private*.ed25519`, or
  bare `private.ed25519`. The repo's `.gitignore` already enforces
  this; treat any pre-commit failure here as a real incident.
- Reuse a CI private key as a developer bringup key (or vice
  versa). Key labels are baked into the signature byte stream
  (`KeyLabel::Ci` vs `KeyLabel::Bringup`); mixing them defeats the
  policy gate.
