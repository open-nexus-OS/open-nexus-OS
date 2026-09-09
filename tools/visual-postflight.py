#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Display-side truth for boot verification (TASK-0080C postflight):
# grabs one frame from the QEMU VNC display (raw RFB, no external deps) and
# judges it — UART markers like `windowd: full-window color visible` are the
# COMPOSITOR's belief; this tool checks what the DISPLAY actually shows, so a
# black-scanout boot can never hide behind green markers (the fake-proof
# class: silent GL-scanout/present failures with an intact marker chain).
# OWNERS: @ui @runtime
# STATUS: Functional
# API_STABILITY: Unstable
# TEST_COVERAGE: exercised by tools/postflight-systemui-bootstrap-shell.sh
#   --visual and the boot-loop triage flow
#
# Usage:
#   tools/visual-postflight.py --out shot.png [--host 127.0.0.1] [--port 5979]
#       [--min-brightness 8.0]
#
# Exit codes: 0 = frame captured and non-black, 1 = frame is (near-)black,
# 2 = capture failed (no VNC display / handshake error).

import argparse
import os
import sys

# No tools/__pycache__: the cargo workspace globs tools/* and a stray
# directory without a Cargo.toml breaks every cargo command (known trap).
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rfb_grab  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5979)
    ap.add_argument("--out", required=True, help="PNG output path")
    ap.add_argument(
        "--min-brightness",
        type=float,
        default=8.0,
        help="mean-luma floor; below = black-scanout verdict",
    )
    ap.add_argument(
        "--min-nonblack-pct",
        type=float,
        default=0.0,
        help="minimum share of non-black pixels (0 = only the luma floor applies)",
    )
    ap.add_argument(
        "--diff-against",
        default="",
        help="PNG of an earlier frame (e.g. the boot splash); the grabbed frame must differ",
    )
    args = ap.parse_args()

    try:
        width, height, fb = rfb_grab.grab_frame(args.host, args.port)
    except (OSError, ConnectionError) as err:
        print(f"visual-postflight: CAPTURE FAILED ({err}) — is the VNC display up "
              f"on {args.host}:{args.port}? (just start-vnc)", file=sys.stderr)
        return 2

    mean, nonblack = rfb_grab.luma_stats(width, height, fb)
    saved = rfb_grab.save_frame(args.out, width, height, fb)
    if args.diff_against:
        try:
            _, _, ref = rfb_grab.load_frame(args.diff_against)
        except (OSError, ValueError, ImportError) as err:
            print(f"visual-postflight: CAPTURE FAILED (reference {args.diff_against}: {err})",
                  file=sys.stderr)
            return 2
        diff = rfb_grab.mean_abs_diff(ref, fb)
        if diff < 2.0:
            print(f"visual-postflight: FAIL — frame is still the reference image (mean abs "
                  f"diff {diff:.2f} vs {args.diff_against}); the desktop never replaced it. "
                  f"Frame: {saved}")
            return 1
    if nonblack < args.min_nonblack_pct:
        print(f"visual-postflight: FAIL — only {nonblack:.1f}% non-black pixels "
              f"(< {args.min_nonblack_pct}%). Frame: {saved}")
        return 1

    if mean < args.min_brightness:
        print(
            f"visual-postflight: FAIL — display is black (mean luma {mean:.1f} < "
            f"{args.min_brightness}). The UART marker chain can still be green here: "
            f"`windowd: full-window color visible` is the compositor's claim, not the "
            f"display's. This is the silent GL-scanout/present class (gpud "
            f"gl_scanout / present lane) — check `gpud: chain G3/G4`, retry the boot "
            f"— and check `gpud: features=` (a gpud built without virgl on a GL "
            f"device is exactly this class, TASK-0324). Frame: {saved}"
        )
        return 1
    print(f"visual-postflight: OK — mean luma {mean:.1f}, frame {width}x{height}: {saved}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
