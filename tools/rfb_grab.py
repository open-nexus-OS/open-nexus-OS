#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The ONE RFB (VNC) frame grab + pixel-statistics helper for display
# truth (TASK-0324 P0). `tools/visual-postflight.py` (one-shot verdict) and
# `tools/pixel_proof_on_marker.py` (marker-triggered snapshots for the
# `visible` lane) both import it — a display claim is checked against what the
# host actually shows, never against the compositor's own markers.
# Dependency-free: raw RFB 3.8, security None, Raw encoding; PNG via Pillow
# when present, else a .bgra dump.
# OWNERS: @ui @runtime
# STATUS: Functional
# TEST_COVERAGE: exercised by the visible lane (scripts/qemu-test.sh pixel_proof)

import socket
import struct


def recvn(sock: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("VNC peer closed mid-message")
        buf += chunk
    return buf


def grab_frame(host: str, port: int, timeout: float = 20.0):
    """RFB 3.8 handshake (security None) + one full Raw framebuffer update.

    Returns (width, height, BGRX bytes).
    """
    s = socket.create_connection((host, port), timeout=timeout)
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
        s.close()
        return width, height, bytes(fb)


def luma_stats(width: int, height: int, fb: bytes, black_below: int = 16):
    """(mean luma 0..255, non-black pixel percentage 0..100)."""
    npx = width * height
    total = 0
    nonblack = 0
    for i in range(0, npx * 4, 4):
        b, g, r = fb[i], fb[i + 1], fb[i + 2]
        luma = (r * 299 + g * 587 + b * 114) // 1000
        total += luma
        if luma >= black_below:
            nonblack += 1
    return total / npx, nonblack * 100.0 / npx


def mean_abs_diff(a: bytes, b: bytes) -> float:
    """Mean absolute per-channel difference of two equally sized BGRX frames."""
    if len(a) != len(b) or not a:
        return 255.0
    total = 0
    for i in range(0, len(a), 4):
        total += abs(a[i] - b[i]) + abs(a[i + 1] - b[i + 1]) + abs(a[i + 2] - b[i + 2])
    return total / (len(a) // 4 * 3)


def save_frame(path: str, width: int, height: int, fb: bytes) -> str:
    """PNG when Pillow is available, else `<path>.bgra`; returns what was written."""
    try:
        from PIL import Image  # type: ignore
        b, g, r, _ = Image.frombytes("RGBA", (width, height), fb).split()
        Image.merge("RGB", (r, g, b)).save(path)
        return path
    except ImportError:
        with open(path + ".bgra", "wb") as f:
            f.write(fb)
        return path + ".bgra"


def load_frame(path: str):
    """Load a frame saved by `save_frame` (PNG via Pillow or .bgra) as (w, h, BGRX)."""
    if path.endswith(".bgra"):
        raise ValueError("raw .bgra frames carry no geometry; compare in-process instead")
    from PIL import Image  # type: ignore
    img = Image.open(path).convert("RGBA")
    r, g, b, a = img.split()
    return img.width, img.height, Image.merge("RGBA", (b, g, r, a)).tobytes()
