#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

"""Capture QEMU's actual display via QMP screendump and report pixel stats.

Proof tool for the GL-scanout path: the guest-side parity readback proves the
render-target CONTENT, this proves what QEMU actually PRESENTS. Prints mean
brightness, non-black pixel ratio, and a coarse 4x4 brightness grid so a
black/flipped/garbled display is distinguishable from a UART-only run.
"""

from __future__ import annotations

import json
import socket
import sys
import time


def qmp_cmd(sock_path: str, cmd: dict) -> dict:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(10.0)
    s.connect(sock_path)
    f = s.makefile("rwb")
    f.readline()  # greeting
    f.write(json.dumps({"execute": "qmp_capabilities"}).encode() + b"\n")
    f.flush()
    f.readline()
    f.write(json.dumps(cmd).encode() + b"\n")
    f.flush()
    deadline = time.time() + 10
    while time.time() < deadline:
        line = f.readline()
        if not line:
            break
        msg = json.loads(line)
        if "return" in msg or "error" in msg:
            s.close()
            return msg
    s.close()
    return {"error": "timeout"}


def read_ppm(path: str):
    with open(path, "rb") as fh:
        data = fh.read()
    if not data.startswith(b"P6"):
        raise ValueError(f"not a P6 ppm: {data[:16]!r}")
    # Parse header: P6 <w> <h> <max>\n then raw RGB.
    parts = []
    idx = 2
    while len(parts) < 3:
        while idx < len(data) and data[idx : idx + 1].isspace():
            idx += 1
        if data[idx : idx + 1] == b"#":
            while data[idx : idx + 1] != b"\n":
                idx += 1
            continue
        start = idx
        while idx < len(data) and not data[idx : idx + 1].isspace():
            idx += 1
        parts.append(int(data[start:idx]))
    idx += 1
    w, h, _maxv = parts
    return w, h, data[idx : idx + w * h * 3]


def main() -> int:
    sock_path = sys.argv[1]
    out = sys.argv[2]
    rsp = qmp_cmd(sock_path, {"execute": "screendump", "arguments": {"filename": out}})
    if "error" in rsp and rsp.get("error"):
        print(f"screendump-error: {rsp}")
        return 2
    w, h, px = read_ppm(out)
    n = w * h
    total = 0
    nonblack = 0
    grid = [[0] * 4 for _ in range(4)]
    counts = [[0] * 4 for _ in range(4)]
    for y in range(0, h, 4):
        for x in range(0, w, 4):
            i = (y * w + x) * 3
            v = px[i] + px[i + 1] + px[i + 2]
            total += v
            if v > 24:
                nonblack += 1
            gy, gx = min(3, y * 4 // h), min(3, x * 4 // w)
            grid[gy][gx] += v
            counts[gy][gx] += 1
    samples = max(1, (h // 4) * (w // 4))
    print(f"size={w}x{h} mean={total / samples / 3:.1f} nonblack={nonblack / samples * 100:.1f}%")
    for gy in range(4):
        row = " ".join(
            f"{grid[gy][gx] / max(1, counts[gy][gx]) / 3:5.1f}" for gx in range(4)
        )
        print(f"grid {row}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
