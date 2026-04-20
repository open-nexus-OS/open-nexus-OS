#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: P5-04 — generate a fresh Ed25519 keypair for the CI label.
# Prints the base64-encoded private seed on stdout (so an operator
# can paste it into a CI-secret store) and writes the public key to
# `keys/evidence-ci.pub.ed25519` (chmod 0644) for repo check-in.
#
# The private key is NEVER persisted to disk by this script. The
# operator is responsible for copying the printed base64 into
# the CI secret named NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64 and for
# closing the terminal scrollback after.
#
# Refuses to overwrite the public key if it already exists; if you
# really want to rotate it, `rm keys/evidence-ci.pub.ed25519` first.
#
# OWNERS: @runtime
# STATUS: Functional (P5-04 surface)

set -Eeuo pipefail

ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "$0")/.." && pwd))
NEBIN="${NEXUS_EVIDENCE_BIN:-}"
if [[ -z "$NEBIN" ]]; then
  NEBIN="$ROOT/target/debug/nexus-evidence"
  if [[ ! -x "$NEBIN" ]]; then
    NEBIN="$ROOT/target/release/nexus-evidence"
  fi
fi
if [[ ! -x "$NEBIN" ]]; then
  echo "$(basename "$0"): nexus-evidence binary not found; run: cargo build -p nexus-evidence" >&2
  exit 2
fi

pub="$ROOT/keys/evidence-ci.pub.ed25519"
if [[ -f "$pub" ]]; then
  echo "$(basename "$0"): refusing to overwrite existing $pub" >&2
  echo "  (delete it explicitly if you really want to rotate the CI key)" >&2
  exit 2
fi
mkdir -p "$ROOT/keys"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
priv="$tmpdir/private.ed25519"

seed_hex=$(head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n')
[[ ${#seed_hex} -eq 64 ]] || {
  echo "$(basename "$0"): /dev/urandom did not yield 32 bytes" >&2
  exit 2
}
"$NEBIN" keygen --seed="$seed_hex" --privkey-out="$priv" --pubkey-out="$pub"
chmod 0600 "$priv"
chmod 0644 "$pub"

if [[ ! -s "$priv" ]]; then
  echo "$(basename "$0"): keygen failed — no private key written" >&2
  exit 2
fi

# The on-disk format is hex (64 ASCII chars + newline). Decode to
# raw 32 bytes and re-encode as base64 so the operator can paste
# the value directly into the CI secret. Prefer openssl for
# portability; fall back to base64(1).
raw="$tmpdir/private.raw"
xxd -r -p < "$priv" > "$raw"
if command -v openssl >/dev/null 2>&1; then
  b64=$(openssl base64 -A < "$raw")
else
  b64=$(base64 -w0 < "$raw" 2>/dev/null || base64 < "$raw" | tr -d '\n')
fi
unset seed_hex

cat >&2 <<EOF
$(basename "$0"): wrote public key to $pub
$(basename "$0"): private key (base64) — copy this into the CI secret
                  named NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64 and then
                  close this terminal:
EOF

printf '%s\n' "$b64"
