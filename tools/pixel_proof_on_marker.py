#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Display-truth actor for the `visible` lane (TASK-0324 P0). Tails
# the uart log and, at each named marker, grabs the host framebuffer over VNC
# (tools/rfb_grab.py) into <out-dir>/<name>.png, then writes
# <out-dir>/display-proof.json with per-snapshot pixel statistics — the
# harness judges the JSON after the run (scripts/qemu-test.sh pixel_proof).
# Two snapshots by contract: `splash` at `gpud: completion wait` (gpud's first
# RAW post-ready line — printed after the bootstrap splash is on the scanout in
# proof AND interactive boots; `gpud: scanout ok` folds away in interactive
# boots) and `desktop` at `systemui: first frame visible` (the compositor's
# claim). The desktop must be non-black AND differ from the splash: markers
# can be green while the screen is black or still the splash.
# Wait-loop doctrine: hard deadline, exit on socket loss, never a babysitter.
#
# Usage: pixel_proof_on_marker.py <vnc-port> <uart-log> <out-dir> <timeout-s>
import json
import os
import sys
import time

# No tools/__pycache__: the cargo workspace globs tools/* and a stray
# directory without a Cargo.toml breaks every cargo command (known trap).
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rfb_grab  # noqa: E402

SNAPSHOTS = [
    ("splash", "gpud: completion wait"),
    ("desktop", "systemui: first frame visible"),
]
# Let the GL flip / host redraw land before sampling the scanout.
SETTLE_S = 0.4


def main() -> int:
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    port, uart, out_dir, timeout_s = int(sys.argv[1]), sys.argv[2], sys.argv[3], float(sys.argv[4])
    os.makedirs(out_dir, exist_ok=True)
    proof = {"snapshots": {}, "status": "incomplete"}
    deadline = time.monotonic() + timeout_s
    pending = list(SNAPSHOTS)
    frames = {}
    pos = 0
    while pending and time.monotonic() < deadline:
        try:
            with open(uart, "rb") as f:
                f.seek(pos)
                chunk = f.read()
                pos += len(chunk)
        except FileNotFoundError:
            time.sleep(0.2)
            continue
        text = chunk.decode("utf-8", "replace")
        hit = [name for name, marker in pending if marker in text]
        for name in hit:
            marker = dict(SNAPSHOTS)[name]
            time.sleep(SETTLE_S)
            try:
                w, h, fb = rfb_grab.grab_frame("127.0.0.1", port)
            except (OSError, ConnectionError) as err:
                proof["snapshots"][name] = {"marker": marker, "error": str(err)}
                proof["status"] = "capture-failed"
                _write(out_dir, proof)
                return 2
            mean, nonblack = rfb_grab.luma_stats(w, h, fb)
            saved = rfb_grab.save_frame(os.path.join(out_dir, f"display-{name}.png"), w, h, fb)
            frames[name] = fb
            entry = {"marker": marker, "file": saved, "width": w, "height": h,
                     "mean_luma": round(mean, 2), "nonblack_pct": round(nonblack, 2)}
            if name == "desktop" and "splash" in frames:
                entry["diff_vs_splash"] = round(rfb_grab.mean_abs_diff(frames["splash"], fb), 2)
            proof["snapshots"][name] = entry
            pending = [(n, m) for n, m in pending if n != name]
            _write(out_dir, proof)
        if pending:
            time.sleep(0.25)
    proof["status"] = "complete" if not pending else "timeout"
    proof["missing"] = [n for n, _ in pending]
    _write(out_dir, proof)
    return 0 if not pending else 1


def _write(out_dir: str, proof: dict) -> None:
    with open(os.path.join(out_dir, "display-proof.json"), "w") as f:
        json.dump(proof, f, indent=2)


if __name__ == "__main__":
    sys.exit(main())
