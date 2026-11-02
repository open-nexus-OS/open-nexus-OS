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
# can be green while the screen is black or still the splash. A third,
# `settled`, at `SELFTEST: ota stage ok` — minutes into the ladder, long after the
# reveal and well before the lane ends: the greeter as it stays, with its clock
# (TASK-0251 finding 1: a band a settled frame showed and the reveal never did).
# A lane may name one more, `NEXUS_PIXEL_PROOF_EXTRA=<name>=<marker>` (TASK-0348: the
# many-windows lane's twelve windows, at `windowd: windows drawn (n=12)`); the judge reads
# the same variable and requires it. Its marker may be an ordered chain `first|then` — the
# snapshot is taken at `then` once `first` was seen — whose step may be a group `a&b`, done once
# every member was seen in any order (RFC-0112: the twelve windows and the shell again, after
# the compositor's restart).
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
    ("settled", "SELFTEST: ota stage ok"),
]


def extra_snapshot():
    """The lane's own snapshot, `(name, marker)` from `NEXUS_PIXEL_PROOF_EXTRA`, or None."""
    spec = os.environ.get("NEXUS_PIXEL_PROOF_EXTRA", "")
    name, sep, marker = spec.partition("=")
    return (name, marker) if sep and name and marker else None


if extra_snapshot():
    SNAPSHOTS.append(extra_snapshot())
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
    # How far each snapshot's chain (`first|then&also`) has come: its step, the step's members seen.
    progress = {name: (0, set()) for name, _ in SNAPSHOTS}
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
        hit = [name for name, marker in pending if chain_done(marker, text, progress, name)]
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
                     "mean_luma": round(mean, 2), "nonblack_pct": round(nonblack, 2),
                     "flat_band_rows": rfb_grab.flat_band_rows(w, h, fb)}
            if name == "desktop" and "splash" in frames:
                entry["diff_vs_splash"] = round(rfb_grab.mean_abs_diff(frames["splash"], fb), 2)
            if name not in ("splash", "desktop", "settled") and "settled" in frames:
                entry["diff_vs_settled"] = round(rfb_grab.mean_abs_diff(frames["settled"], fb), 2)
            proof["snapshots"][name] = entry
            pending = [(n, m) for n, m in pending if n != name]
            _write(out_dir, proof)
        if pending:
            time.sleep(0.25)
    proof["status"] = "complete" if not pending else "timeout"
    proof["missing"] = [n for n, _ in pending]
    _write(out_dir, proof)
    return 0 if not pending else 1


def chain_done(marker: str, text: str, progress: dict, name: str) -> bool:
    """Advances `name`'s chain (`first|then|…`, in order; a step `a&b` is done once each member
    was seen, in any order) through `text`; True at its end."""
    steps = [step.split("&") for step in marker.split("|")]
    step, seen = progress[name]
    at = 0
    while step < len(steps):
        ends = []
        for member in steps[step]:
            if member in seen:
                continue
            k = text.find(member, at)
            if k >= 0:
                seen.add(member)
                ends.append(k + len(member))
        if not all(member in seen for member in steps[step]):
            break
        at = max(ends, default=at)
        step, seen = step + 1, set()
    progress[name] = (step, seen)
    return step >= len(steps)


def _write(out_dir: str, proof: dict) -> None:
    with open(os.path.join(out_dir, "display-proof.json"), "w") as f:
        json.dump(proof, f, indent=2)


if __name__ == "__main__":
    sys.exit(main())
