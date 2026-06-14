#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

"""Minimal RFB (VNC) framebuffer grab — proof tool for GL scanout output.

Connects to a QEMU VNC server (security none), requests one full
FramebufferUpdate in raw encoding, and reports pixel statistics (and
optionally writes a PPM). This sees exactly what a viewer would see —
including GL-texture scanouts that QMP `screendump` cannot capture
("no surface").
"""

from __future__ import annotations

import socket
import struct
import sys


def recv_exact(s: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = s.recv(n - len(buf))
        if not chunk:
            raise EOFError(f"short read: wanted {n}, got {len(buf)}")
        buf += chunk
    return buf


def main() -> int:
    host, port = "127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 5977
    out = sys.argv[2] if len(sys.argv) > 2 else ""
    s = socket.create_connection((host, port), timeout=15)
    s.settimeout(20)

    server_ver = recv_exact(s, 12)  # e.g. b"RFB 003.008\n"
    s.sendall(b"RFB 003.008\n")
    nsec = recv_exact(s, 1)[0]
    if nsec == 0:
        reason_len = struct.unpack(">I", recv_exact(s, 4))[0]
        raise RuntimeError(recv_exact(s, reason_len).decode())
    sec_types = recv_exact(s, nsec)
    if 1 not in sec_types:
        raise RuntimeError(f"need security=none, server offers {list(sec_types)}")
    s.sendall(bytes([1]))
    result = struct.unpack(">I", recv_exact(s, 4))[0]
    if result != 0:
        raise RuntimeError("security handshake failed")
    s.sendall(bytes([1]))  # ClientInit: shared

    # ServerInit: width, height, pixel format (16 bytes), name.
    w, h = struct.unpack(">HH", recv_exact(s, 4))
    pf = recv_exact(s, 16)
    bpp, depth, big_endian, true_color = pf[0], pf[1], pf[2], pf[3]
    rmax, gmax, bmax = struct.unpack(">HHH", pf[4:10])
    rsh, gsh, bsh = pf[10], pf[11], pf[12]
    name_len = struct.unpack(">I", recv_exact(s, 4))[0]
    recv_exact(s, name_len)
    print(
        f"server={server_ver.strip().decode()} fb={w}x{h} bpp={bpp} depth={depth} "
        f"shifts=({rsh},{gsh},{bsh})"
    )

    # SetEncodings: raw only.
    s.sendall(struct.pack(">BxH i", 2, 1, 0))
    # FramebufferUpdateRequest: non-incremental, full screen.
    s.sendall(struct.pack(">BBHHHH", 3, 0, 0, 0, w, h))

    bytes_pp = bpp // 8
    fb = bytearray(w * h * 4)  # RGBX
    got_pixels = 0
    while got_pixels < w * h:
        mtype = recv_exact(s, 1)[0]
        if mtype != 0:  # not FramebufferUpdate (e.g. bell/cuttext) — skip
            if mtype == 2:
                continue
            if mtype == 3:
                ln = struct.unpack(">I", recv_exact(s, 7)[3:])[0]
                recv_exact(s, ln)
                continue
            raise RuntimeError(f"unexpected message type {mtype}")
        recv_exact(s, 1)
        (nrects,) = struct.unpack(">H", recv_exact(s, 2))
        for _ in range(nrects):
            rx, ry, rw, rh, enc = struct.unpack(">HHHHi", recv_exact(s, 12))
            if enc != 0:
                raise RuntimeError(f"unexpected encoding {enc}")
            data = recv_exact(s, rw * rh * bytes_pp)
            for row in range(rh):
                for col in range(rw):
                    off = (row * rw + col) * bytes_pp
                    if bytes_pp == 4:
                        v = struct.unpack(
                            "<I" if not big_endian else ">I", data[off : off + 4]
                        )[0]
                    else:
                        v = struct.unpack(
                            "<H" if not big_endian else ">H", data[off : off + 2]
                        )[0]
                    r = (v >> rsh) & rmax
                    g = (v >> gsh) & gmax
                    b = (v >> bsh) & bmax
                    r = r * 255 // max(1, rmax)
                    g = g * 255 // max(1, gmax)
                    b = b * 255 // max(1, bmax)
                    fo = ((ry + row) * w + rx + col) * 4
                    fb[fo : fo + 3] = bytes((r, g, b))
            got_pixels += rw * rh
        if true_color == 0:
            raise RuntimeError("palette mode unsupported")

    # Stats: mean brightness, non-black ratio, 4x4 grid.
    total = nonblack = 0
    grid = [[0] * 4 for _ in range(4)]
    counts = [[0] * 4 for _ in range(4)]
    for y in range(0, h, 4):
        for x in range(0, w, 4):
            i = (y * w + x) * 4
            v = fb[i] + fb[i + 1] + fb[i + 2]
            total += v
            if v > 24:
                nonblack += 1
            gy, gx = min(3, y * 4 // h), min(3, x * 4 // w)
            grid[gy][gx] += v
            counts[gy][gx] += 1
    samples = max(1, (h // 4) * (w // 4))
    print(f"mean={total / samples / 3:.1f} nonblack={nonblack / samples * 100:.1f}%")
    for gy in range(4):
        print(
            "grid "
            + " ".join(f"{grid[gy][gx] / max(1, counts[gy][gx]) / 3:5.1f}" for gx in range(4))
        )
    if out:
        with open(out, "wb") as fh:
            fh.write(b"P6\n%d %d\n255\n" % (w, h))
            for i in range(0, w * h * 4, 4):
                fh.write(fb[i : i + 3])
        print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
