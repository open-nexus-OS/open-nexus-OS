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
import socket
import struct
import sys


def recvn(sock: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("VNC peer closed mid-message")
        buf += chunk
    return buf


def grab_frame(host: str, port: int):
    """RFB 3.8 handshake (security None) + one full Raw framebuffer update.

    Returns (width, height, BGRX bytes).
    """
    s = socket.create_connection((host, port), timeout=20)
    recvn(s, 12)  # server version
    s.sendall(b"RFB 003.008\n")
    ntypes = recvn(s, 1)[0]
    types = recvn(s, ntypes)
    if 1 not in types:
        raise ConnectionError(f"VNC auth types {list(types)} (need None)")
    s.sendall(bytes([1]))
    if struct.unpack(">I", recvn(s, 4))[0] != 0:
        raise ConnectionError("VNC security handshake failed")
    s.sendall(bytes([1]))  # ClientInit (shared)
    width, height = struct.unpack(">HH", recvn(s, 4))
    recvn(s, 16)  # server pixel format (we override)
    recvn(s, struct.unpack(">I", recvn(s, 4))[0])  # desktop name
    # 32bpp truecolor, shifts 16/8/0 → little-endian BGRX in memory.
    s.sendall(struct.pack(">BxxxBBBBHHHBBBxxx", 0, 32, 24, 0, 1, 255, 255, 255, 16, 8, 0))
    s.sendall(struct.pack(">BxH i", 2, 1, 0))  # SetEncodings: Raw
    s.sendall(struct.pack(">BBHHHH", 3, 0, 0, 0, width, height))
    fb = bytearray(width * height * 4)
    while True:
        if recvn(s, 1)[0] != 0:  # FramebufferUpdate
            continue
        recvn(s, 1)
        nrect = struct.unpack(">H", recvn(s, 2))[0]
        for _ in range(nrect):
            x, y, rw, rh, enc = struct.unpack(">HHHHi", recvn(s, 12))
            if enc != 0:
                raise ConnectionError(f"unexpected encoding {enc}")
            data = recvn(s, rw * rh * 4)
            for row in range(rh):
                off = ((y + row) * width + x) * 4
                fb[off:off + rw * 4] = data[row * rw * 4:(row + 1) * rw * 4]
        return width, height, bytes(fb)


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
    args = ap.parse_args()

    try:
        width, height, fb = grab_frame(args.host, args.port)
    except (OSError, ConnectionError) as err:
        print(f"visual-postflight: CAPTURE FAILED ({err}) — is the VNC display up "
              f"on {args.host}:{args.port}? (just start-vnc)", file=sys.stderr)
        return 2

    # Mean luma over BGRX without external deps.
    total = 0
    npx = width * height
    for i in range(0, npx * 4, 4):
        b, g, r = fb[i], fb[i + 1], fb[i + 2]
        total += (r * 299 + g * 587 + b * 114) // 1000
    mean = total / npx

    try:
        from PIL import Image
        b, g, r, _ = Image.frombytes("RGBA", (width, height), fb).split()
        Image.merge("RGB", (r, g, b)).save(args.out)
        saved = args.out
    except ImportError:
        with open(args.out + ".bgra", "wb") as f:
            f.write(fb)
        saved = args.out + ".bgra (install Pillow for PNG)"

    if mean < args.min_brightness:
        print(
            f"visual-postflight: FAIL — display is black (mean luma {mean:.1f} < "
            f"{args.min_brightness}). The UART marker chain can still be green here: "
            f"`windowd: full-window color visible` is the compositor's claim, not the "
            f"display's. This is the silent GL-scanout/present class (gpud "
            f"gl_scanout / present lane) — check `gpud: chain G3/G4`, retry the boot "
            f"(known intermittent), and see the scanout-readback task in "
            f"tasks/TRACK-OPEN-POINTS-2026-07.md. Frame: {saved}"
        )
        return 1
    print(f"visual-postflight: OK — mean luma {mean:.1f}, frame {width}x{height}: {saved}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
