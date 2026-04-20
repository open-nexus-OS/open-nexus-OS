#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Thin wrapper around `nexus-evidence verify`. Resolves
# the public key based on the requested policy (P5-03):
#
#   --policy=ci      pubkey from <repo-root>/keys/evidence-ci.pub.ed25519
#   --policy=bringup pubkey from
#                    ~/.config/nexus/bringup-key/public.ed25519
#                    (created alongside private.ed25519 by
#                    tools/gen-bringup-key.sh in P5-04)
#   --policy=any     accepts either label; tries CI key first, falls
#                    back to bringup
#
# Exit codes mirror `nexus-evidence verify`:
#   0  signature ok and policy satisfied
#   1  signature mismatch / label mismatch / signature missing
#   2  missing key material or input file
#
# OWNERS: @runtime
# STATUS: Functional (P5-03 surface)

set -Eeuo pipefail

usage() {
  cat >&2 <<EOF
usage: $(basename "$0") <bundle.tar.gz> [--policy=ci|bringup|any]

  Verifies a sealed evidence bundle. Default policy: any.
EOF
  exit 2
}

bundle=""
policy="any"
for arg in "$@"; do
  case "$arg" in
    --policy=*) policy="${arg#--policy=}" ;;
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
[[ -f "$bundle" ]] || { echo "$(basename "$0"): bundle not found: $bundle" >&2; exit 2; }

ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "$0")/.." && pwd))
NEBIN="${NEXUS_EVIDENCE_BIN:-}"
if [[ -z "$NEBIN" ]]; then
  NEBIN="$ROOT/target/debug/nexus-evidence"
  if [[ ! -x "$NEBIN" ]]; then
    NEBIN="$ROOT/target/release/nexus-evidence"
  fi
fi
[[ -x "$NEBIN" ]] || { echo "$(basename "$0"): nexus-evidence binary not found; run: cargo build -p nexus-evidence" >&2; exit 2; }

CI_PUB="${NEXUS_EVIDENCE_CI_PUBKEY:-$ROOT/keys/evidence-ci.pub.ed25519}"
BR_PUB="${NEXUS_EVIDENCE_BRINGUP_PUBKEY:-$HOME/.config/nexus/bringup-key/public.ed25519}"

try_verify() {
  local pubkey="$1"
  local pol="$2"
  if [[ ! -f "$pubkey" ]]; then
    return 2
  fi
  if [[ "$pol" == "any" ]]; then
    "$NEBIN" verify "$bundle" --pubkey="$pubkey"
  else
    "$NEBIN" verify "$bundle" --pubkey="$pubkey" --policy="$pol"
  fi
}

case "$policy" in
  ci)
    [[ -f "$CI_PUB" ]] || { echo "$(basename "$0"): CI pubkey missing at $CI_PUB" >&2; exit 2; }
    exec "$NEBIN" verify "$bundle" --pubkey="$CI_PUB" --policy=ci
    ;;
  bringup)
    [[ -f "$BR_PUB" ]] || { echo "$(basename "$0"): bringup pubkey missing at $BR_PUB" >&2; exit 2; }
    exec "$NEBIN" verify "$bundle" --pubkey="$BR_PUB" --policy=bringup
    ;;
  any)
    if [[ -f "$CI_PUB" ]] && try_verify "$CI_PUB" any 2>/dev/null; then
      exit 0
    fi
    if [[ -f "$BR_PUB" ]] && try_verify "$BR_PUB" any 2>/dev/null; then
      exit 0
    fi
    echo "$(basename "$0"): bundle did not verify under any known pubkey (tried $CI_PUB and $BR_PUB)" >&2
    exit 1
    ;;
  *)
    echo "$(basename "$0"): unknown --policy=$policy (want ci|bringup|any)" >&2
    usage
    ;;
esac
