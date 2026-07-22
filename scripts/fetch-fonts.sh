#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Fetches the PINNED Noto Sans CJK faces (font-library.md contract:
# Inter primary + Noto Sans JP/KR/SC fallback, SIL OFL 1.1) into
# resources/fonts/noto/. The notofonts/noto-cjk repo is far too large for a
# submodule (multi-GB); instead each OTF is downloaded RAW at a pinned
# commit and verified against a pinned SHA-256 — same determinism, ~50 MB.
# The OTFs are BUILD inputs only (nexus-text-baked bakes A8 atlases); they
# never ship in the image and stay untracked (resources/fonts/noto/ is
# gitignored except for this pin's bookkeeping).
#
# Usage: scripts/fetch-fonts.sh   (no-op when everything verifies)

set -euo pipefail

REF="f8d157532fbfaeda587e826d4cd5b21a49186f7c" # notofonts/noto-cjk, 2026-07 pin
BASE="https://raw.githubusercontent.com/notofonts/noto-cjk/${REF}/Sans/OTF"
DEST="$(cd "$(dirname "$0")/.." && pwd)/resources/fonts/noto"

# file  subpath  sha256
FONTS=(
  "NotoSansCJKjp-Regular.otf Japanese/NotoSansCJKjp-Regular.otf 68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5"
  "NotoSansCJKkr-Regular.otf Korean/NotoSansCJKkr-Regular.otf 6bcb2a0703aa137e874fc2dffa85f6c21ba9a67fa329e81b8c801663af7e992a"
  "NotoSansCJKsc-Regular.otf SimplifiedChinese/NotoSansCJKsc-Regular.otf 2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b"
)

mkdir -p "$DEST"
for entry in "${FONTS[@]}"; do
  read -r name subpath sha <<<"$entry"
  target="$DEST/$name"
  if [[ -f "$target" ]] && echo "$sha  $target" | sha256sum -c --quiet - 2>/dev/null; then
    echo "[ok]   $name (pinned)"
    continue
  fi
  echo "[get]  $name"
  curl -sfL "$BASE/$subpath" -o "$target.tmp"
  echo "$sha  $target.tmp" | sha256sum -c --quiet - || {
    rm -f "$target.tmp"
    echo "[FAIL] $name: checksum mismatch (pin drift?)" >&2
    exit 1
  }
  mv "$target.tmp" "$target"
done
echo "[done] Noto Sans CJK faces pinned in $DEST"
