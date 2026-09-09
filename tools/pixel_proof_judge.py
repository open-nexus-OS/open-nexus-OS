#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The verdict half of the display-truth proof (TASK-0324 P0): reads
# the display-proof.json that tools/pixel_proof_on_marker.py wrote during the
# run and decides. Contract: a `splash` and a `desktop` snapshot exist, the
# desktop is non-black (>= MIN_NONBLACK_PCT of pixels above the black floor)
# and differs from the splash (mean abs diff >= MIN_DIFF). Exit 0 = proven.
#
# Usage: pixel_proof_judge.py <display-proof.json>
import json
import sys

MIN_NONBLACK_PCT = 30.0
MIN_DIFF = 2.0


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
    if d["nonblack_pct"] < MIN_NONBLACK_PCT:
        print(f"[error] PIXEL PROOF: desktop is black ({d['nonblack_pct']}% non-black, "
              f"mean luma {d['mean_luma']}) — {d['file']}", file=sys.stderr)
        ok = False
    if d.get("diff_vs_splash", 255.0) < MIN_DIFF:
        print(f"[error] PIXEL PROOF: desktop snapshot is still the splash "
              f"(diff {d.get('diff_vs_splash')}) — {d['file']}", file=sys.stderr)
        ok = False
    if ok:
        print(f"[info] PIXEL PROOF ok: desktop {d['nonblack_pct']}% non-black, luma "
              f"{d['mean_luma']}, diff vs splash {d.get('diff_vs_splash')} — {d['file']}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
