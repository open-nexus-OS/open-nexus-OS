#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: P5-04 — generate a fresh Ed25519 keypair for the local
# bringup label. Writes the 32-byte private seed to
# ~/.config/nexus/bringup-key/private.ed25519 (chmod 0600) and the
# 32-byte public key to ~/.config/nexus/bringup-key/public.ed25519
# (chmod 0644).
#
# This script does NOT install the public key into `keys/`. The
# bringup label is intentionally NOT trusted by the CI gate — only
# `keys/evidence-ci.pub.ed25519` is. If a maintainer wants to publish
# their bringup pubkey for ad-hoc cross-verification, they copy it
# manually with a clearly-named file (e.g. `keys/bringup-<handle>.pub.ed25519`).
#
# Refuses to overwrite an existing key (callers must rm + rerun).
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

dir="${NEXUS_EVIDENCE_BRINGUP_DIR:-$HOME/.config/nexus/bringup-key}"
priv="$dir/private.ed25519"
pub="$dir/public.ed25519"

if [[ -f "$priv" ]]; then
  echo "$(basename "$0"): refusing to overwrite existing $priv" >&2
  echo "  (delete it explicitly if you really want a fresh keypair)" >&2
  exit 2
fi

mkdir -p "$dir"
chmod 0700 "$dir"

# `nexus-evidence keygen` requires a 32-byte seed in hex; we draw
# fresh entropy from /dev/urandom and pass it through stdin so the
# seed never lands in `ps`/shell history.
seed_hex=$(head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n')
[[ ${#seed_hex} -eq 64 ]] || {
  echo "$(basename "$0"): /dev/urandom did not yield 32 bytes" >&2
  exit 2
}
"$NEBIN" keygen --seed="$seed_hex" --privkey-out="$priv" --pubkey-out="$pub"
unset seed_hex
chmod 0600 "$priv"
chmod 0644 "$pub"

echo "$(basename "$0"): wrote bringup keypair:" >&2
echo "  private: $priv (0600)" >&2
echo "  public:  $pub (0644)" >&2
