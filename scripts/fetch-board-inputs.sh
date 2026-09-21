#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Fetches the PINNED vendor boot pieces for the reference board
#          (TASK-0327): the first-stage loader (SPL, DDR init), OpenSBI, the
#          vendor U-Boot (the host-side FLASHING vehicle — it runs in RAM
#          during `just board-flash`), the boot-ROM `bootinfo` headers and the
#          partition table the vendor's own flasher uses. They ship only inside
#          the vendor's release archive (~250 MB for ~2.4 MB of pieces), so the
#          archive is downloaded once, verified against a pinned SHA-256, the
#          pieces extracted and each verified against its own pin, and the
#          archive kept in a cache so a re-run is a checksum, not a download.
#          Everything lands in resources/board/<board>/vendor/ (gitignored);
#          resources/board/<board>/PROVENANCE.md (committed) is the human copy
#          of the pins, versions and licenses.
#
#          Why pinned binaries and not a source build: SPL/U-Boot/OpenSBI are
#          the vendor's builds of open-source programs; until Block 1 decides
#          which stages stay in OUR boot chain (ADR-0066) they are inputs, not
#          sources — the same rule as the pinned fonts. Vendor binaries are
#          never committed and never linked to our code.
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable (paths under resources/board/<board>/vendor/ are the contract)
# TEST_COVERAGE: --check (used by scripts/check-deps.sh); TASK-0327 T2 against the desk board
# DEPENDS_ON: curl, python3 (zipfile — no `unzip` needed), sha256sum
#
# Usage:
#   scripts/fetch-board-inputs.sh           # fetch + verify (no-op once everything verifies)
#   scripts/fetch-board-inputs.sh --check   # report only, exit 3 if a piece is missing/unverified
#
# Exit codes: 0 pieces present and verified · 1 download/verify failed · 3 --check: missing

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOARD="bpi-f3"
DEST="$ROOT/resources/board/$BOARD/vendor"
CACHE="$DEST/.cache"

# --- the pin (2026-09-21) ----------------------------------------------------
# Vendor OS release "Minimal" v2.3.5 for the k1x SoC family — the smallest
# archive that carries the boot pieces. The server publishes an MD5 next to
# the archive; we verified it on fetch and pin SHA-256 ourselves.
ARCHIVE_URL="http://archive.spacemit.com/image/k1/version/bianbu/v2.3.5/Bianbu-Minimal-K1-V2.3.5-20260601180942.zip"
ARCHIVE_NAME="Bianbu-Minimal-K1-V2.3.5-20260601180942.zip"
ARCHIVE_SHA256="364508121016d452295105894c493362dac594c5b3f61e162b782f46885d9bed"

# path-in-archive  sha256  (what it is)
PIECES=(
  "factory/FSBL.bin           980c0bca9711f1e59a4d52aed565b4dc1e240134818224a8c88f60b2229b0fad"
  "factory/bootinfo_sd.bin    f339e3e576ce94d7812c2887622c5882ae12ac3c9a98059aef37741850a43cb6"
  "factory/bootinfo_emmc.bin  5433619a573ec7935450f303168b87c97dc5511905291328df40a2ff081b3d07"
  "fw_dynamic.itb             ef46cb0bc6ec2daf813fa968c0613e80999ba2b67d4bba49445ec31ed62ba52d"
  "u-boot.itb                 124c30cb568f83f05c25a591e14b3c1f2ec3f281c362c43f73d27f305a31f3ee"
  "env.bin                    f91f8d85420e54329cd0ecf97239bebeb22f6d058c7ea2a95a08d9de10681f90"
  "partition_universal.json   568d9848097c72ba01ded40534022d35be7dda57ece13863194dd3a442d9568e"
  "fastboot.yaml              95642b765cff5b7c6e2439e44b1b7ea70a77fd79ced3e37a907185f383545278"
)

CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --check) CHECK_ONLY=1 ;;
    -h|--help) sed -n '28,32p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

log() { printf '\033[1;34m[board-inputs]\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m[board-inputs][error]\033[0m %s\n' "$*" >&2; }

verified() { # path sha
  [ -f "$1" ] && echo "$2  $1" | sha256sum -c --quiet - 2>/dev/null
}

missing=0
for entry in "${PIECES[@]}"; do
  read -r sub sha <<<"$entry"
  verified "$DEST/$sub" "$sha" || missing=$((missing + 1))
done

if [ "$missing" -eq 0 ]; then
  log "vendor boot pieces present and pinned in $DEST (${#PIECES[@]} files)"
  exit 0
fi
if [ "$CHECK_ONLY" = 1 ]; then
  err "$missing of ${#PIECES[@]} vendor boot pieces missing or unverified in $DEST"
  err "fix: scripts/fetch-board-inputs.sh   (downloads the ~250 MB vendor archive once)"
  exit 3
fi

mkdir -p "$DEST/factory" "$CACHE"
archive="$CACHE/$ARCHIVE_NAME"
if ! verified "$archive" "$ARCHIVE_SHA256"; then
  log "downloading $ARCHIVE_NAME (~250 MB, once; cached in $CACHE)"
  curl -fL --retry 3 --progress-bar -o "$archive.tmp" "$ARCHIVE_URL"
  if ! verified "$archive.tmp" "$ARCHIVE_SHA256"; then
    rm -f "$archive.tmp"
    err "archive checksum mismatch — the pin drifted or the download is corrupt; nothing extracted"
    exit 1
  fi
  mv "$archive.tmp" "$archive"
fi

# Extract only the pinned pieces; python's zipfile is on every supported host
# (python3 is a CORE package), `unzip` is not.
python3 - "$archive" "$DEST" "${PIECES[@]}" <<'PY'
import sys, zipfile, os
archive, dest, *pieces = sys.argv[1:]
with zipfile.ZipFile(archive) as z:
    for entry in pieces:
        sub = entry.split()[0]
        target = os.path.join(dest, sub)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with z.open(sub) as src, open(target, "wb") as dst:
            dst.write(src.read())
PY

bad=0
for entry in "${PIECES[@]}"; do
  read -r sub sha <<<"$entry"
  if verified "$DEST/$sub" "$sha"; then
    log "[ok]  $sub"
  else
    err "[FAIL] $sub: checksum mismatch after extraction"
    rm -f "$DEST/$sub"
    bad=1
  fi
done
[ "$bad" -eq 0 ] || exit 1
log "vendor boot pieces ready: $DEST  (pins + licenses: resources/board/$BOARD/PROVENANCE.md)"
