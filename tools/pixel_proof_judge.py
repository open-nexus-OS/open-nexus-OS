#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The verdict half of the display-truth proof (TASK-0324 P0): reads
# the display-proof.json that tools/pixel_proof_on_marker.py wrote during the
# run and decides. Contract: a `splash` and a `desktop` snapshot exist, the
# desktop is non-black (>= MIN_NONBLACK_PCT of pixels above the black floor),
# has no unpainted band (< MAX_FLAT_BAND_ROWS flat rows in a row) and differs from
# the splash (mean abs diff >= MIN_DIFF). Exit 0 = proven.
#
# Usage: pixel_proof_judge.py <display-proof.json>
import json
import sys

# "Not black" = enough pixels above rfb_grab's black floor (luma >= 16/255) AND a
# mean luma clearly above zero. A black scanout — the class this proof exists for
# (a 2D gpud on a GL device: every marker green, every pixel 0) — measures ~0 % / ~0.
#
# The floor used to be 30 %. That number was calibrated on a frame the old
# heuristic reveal LEAKED: gpud revealed on a self-tick before windowd's first
# present, so the capture showed the bare wallpaper (bright, no greeter) — the
# "wallpaper-first" flash ADR-0041's atomic reveal forbids. With the reveal
# handshake (TASK-0324 P6-c) the first revealed frame is the honest one — the
# greeter dimming that wallpaper — and measures ~15 % / luma ~10. Thresholds sit
# 3-5x above black and 3-5x below the honest frame, so neither class is ambiguous.
MIN_NONBLACK_PCT = 5.0
MIN_MEAN_LUMA = 2.0
MIN_DIFF = 2.0
# An unpainted band: this many consecutive rows each one flat colour across the whole width.
# A desktop varies along every row (wallpaper, chrome, text); the damage grid that stopped at
# row 832 of a 1080-row display left a 240-row flat band (TASK-0251 P2a step 3). One damage
# tile is 64 rows.
MAX_FLAT_BAND_ROWS = 64


def main() -> int:
    proof = json.load(open(sys.argv[1]))
    snaps = proof.get("snapshots", {})
    ok = True
    for name in ("splash", "desktop"):
        if name not in snaps or "error" in snaps[name]:
            why = snaps.get(name, {}).get("error", "marker never seen")
            print(f"[error] PIXEL PROOF: no '{name}' snapshot ({why})", file=sys.stderr)
            ok = False
    if not ok:
        return 1
    d = snaps["desktop"]
    if d["nonblack_pct"] < MIN_NONBLACK_PCT or d["mean_luma"] < MIN_MEAN_LUMA:
        # Every metric, every time: a verdict that hides the other numbers costs the next
        # reader a trip into the JSON (diff was 31.67 on the run that motivated this).
        print(f"[error] PIXEL PROOF: desktop is black ({d['nonblack_pct']}% non-black, "
              f"mean luma {d['mean_luma']}, diff vs splash {d.get('diff_vs_splash')}) — "
              f"{d['file']}", file=sys.stderr)
        ok = False
    if d.get("flat_band_rows", 0) >= MAX_FLAT_BAND_ROWS:
        print(f"[error] PIXEL PROOF: desktop has an unpainted band of {d['flat_band_rows']} "
              f"flat rows — {d['file']}", file=sys.stderr)
        ok = False
    if d.get("diff_vs_splash", 255.0) < MIN_DIFF:
        print(f"[error] PIXEL PROOF: desktop snapshot is still the splash "
              f"(diff {d.get('diff_vs_splash')}) — {d['file']}", file=sys.stderr)
        ok = False
    if ok:
        print(f"[info] PIXEL PROOF ok: desktop {d['nonblack_pct']}% non-black, luma "
              f"{d['mean_luma']}, diff vs splash {d.get('diff_vs_splash')}, flat band "
              f"{d.get('flat_band_rows')} rows — {d['file']}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
