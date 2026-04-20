#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Thin wrapper around `nexus-evidence seal`. Resolves the
# private key based on the requested label (P5-03):
#
#   --label=ci      private key from $NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64
#                   (env var; never persisted to disk in CI)
#   --label=bringup private key from
#                   ~/.config/nexus/bringup-key/private.ed25519
#                   (chmod 0600; created by tools/gen-bringup-key.sh
#                   in P5-04)
#
# Refuses to run if the resolved key source is missing — callers
# get a clean exit code rather than a half-sealed bundle.
#
# OWNERS: @runtime
# STATUS: Functional (P5-03 surface; gen-bringup-key.sh + secret
#         scanner land in P5-04)

set -Eeuo pipefail

usage() {
  cat >&2 <<EOF
usage: $(basename "$0") <bundle.tar.gz> --label=ci|bringup

  Reads private key material from the appropriate source for the
  given label, then invokes \`nexus-evidence seal\` to add a
  signature.bin to the bundle in-place.

  --label=ci      reads \$NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64
  --label=bringup reads ~/.config/nexus/bringup-key/private.ed25519
EOF
  exit 2
}

bundle=""
label=""
for arg in "$@"; do
  case "$arg" in
    --label=*) label="${arg#--label=}" ;;
    -h|--help) usage ;;
    -*) echo "$(basename "$0"): unknown flag $arg" >&2; usage ;;
    *)
      if [[ -z "$bundle" ]]; then
        bundle="$arg"
      else
        echo "$(basename "$0"): only one bundle path accepted" >&2
        usage
      fi
      ;;
  esac
done

[[ -z "$bundle" ]] && usage
[[ -z "$label" ]] && usage
[[ -f "$bundle" ]] || { echo "$(basename "$0"): bundle not found: $bundle" >&2; exit 2; }

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

# Resolve private key into a tempfile so the CLI sees a path. The
# tempfile is cleaned up unconditionally on exit so a CI-env private
# key never lingers on the runner's disk.
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
priv="$tmpdir/private.ed25519"

case "$label" in
  ci)
    if [[ -z "${NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64:-}" ]]; then
      echo "$(basename "$0"): \$NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64 is unset (required for --label=ci)" >&2
      exit 2
    fi
    printf '%s' "$NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64" > "$priv"
    chmod 0600 "$priv"
    ;;
  bringup)
    src="${NEXUS_EVIDENCE_BRINGUP_PRIVKEY:-$HOME/.config/nexus/bringup-key/private.ed25519}"
    if [[ ! -f "$src" ]]; then
      echo "$(basename "$0"): bringup private key missing at $src" >&2
      echo "  (P5-04 will provide tools/gen-bringup-key.sh; for now create it manually)" >&2
      exit 2
    fi
    perms=$(stat -c '%a' "$src" 2>/dev/null || stat -f '%Lp' "$src")
    if [[ "$perms" != "600" ]]; then
      echo "$(basename "$0"): bringup private key $src has perms $perms; refusing (want 0600)" >&2
      exit 2
    fi
    cp "$src" "$priv"
    chmod 0600 "$priv"
    ;;
  *)
    echo "$(basename "$0"): unknown --label=$label (want ci|bringup)" >&2
    exit 2
    ;;
esac

exec "$NEBIN" seal "$bundle" --privkey="$priv" --label="$label"
