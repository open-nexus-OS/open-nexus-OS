#!/usr/bin/env python3
"""Minimal RFB (VNC) framebuffer grabber for visual boot verification.

Connects to QEMU's -vnc display, negotiates security None, requests one full
RAW framebuffer update and writes it as a PPM:

    python3 tools/rfb_screenshot.py 127.0.0.1 5979 /tmp/frame.ppm

(QMP screendump reports "no surface" under egl-headless + virgl; this works.)
"""
import socket, struct, sys

def recv_exact(s, n):
    buf = b""
    while len(buf) < n:
        chunk = s.recv(n - len(buf))
        if not chunk:
            raise RuntimeError(f"eof (wanted {n}, got {len(buf)})")
        buf += chunk
    return buf

def main():
    host, port, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    s = socket.create_connection((host, port), timeout=20)
    s.settimeout(20)
    ver = recv_exact(s, 12)
    s.sendall(b"RFB 003.008\n")
    (ntypes,) = struct.unpack("B", recv_exact(s, 1))
    types = recv_exact(s, ntypes)
    if 1 not in types:
        raise RuntimeError(f"no security None offered: {list(types)}")
    s.sendall(bytes([1]))
    (secres,) = struct.unpack(">I", recv_exact(s, 4))
    if secres != 0:
        raise RuntimeError("security failed")
    s.sendall(bytes([1]))  # ClientInit: shared
    w, h = struct.unpack(">HH", recv_exact(s, 4))
    pf = recv_exact(s, 16)
    bpp, depth, big_endian, true_color = pf[0], pf[1], pf[2], pf[3]
    rmax, gmax, bmax = struct.unpack(">HHH", pf[4:10])
    rsh, gsh, bsh = pf[10], pf[11], pf[12]
    (namelen,) = struct.unpack(">I", recv_exact(s, 4))
    recv_exact(s, namelen)
    print(f"fb {w}x{h} bpp={bpp} depth={depth} be={big_endian} tc={true_color} "
          f"shifts=({rsh},{gsh},{bsh}) max=({rmax},{gmax},{bmax})")
    # SetEncodings: Raw only
    s.sendall(struct.pack(">BBH i", 2, 0, 1, 0))
    # FramebufferUpdateRequest: full, non-incremental
    s.sendall(struct.pack(">BBHHHH", 3, 0, 0, 0, w, h))
    img = bytearray(w * h * 3)
    got = 0
    while got < w * h:
        mtype = recv_exact(s, 1)[0]
        if mtype != 0:
            # skip other server messages minimally (bell=2 none, cuttext=3)
            if mtype == 2:
                continue
            if mtype == 3:
                recv_exact(s, 3)
                (l,) = struct.unpack(">I", recv_exact(s, 4))
                recv_exact(s, l)
                continue
            if mtype == 1:  # SetColourMapEntries
                recv_exact(s, 1)
                first, ncols = struct.unpack(">HH", recv_exact(s, 4))
                recv_exact(s, ncols * 6)
                continue
            raise RuntimeError(f"unexpected msg {mtype}")
        recv_exact(s, 1)
        (nrects,) = struct.unpack(">H", recv_exact(s, 2))
        for _ in range(nrects):
            x, y, rw, rh, enc = struct.unpack(">HHHHi", recv_exact(s, 12))
            if enc != 0:
                raise RuntimeError(f"non-raw encoding {enc}")
            bypp = bpp // 8
            data = recv_exact(s, rw * rh * bypp)
            for row in range(rh):
                for col in range(rw):
                    off = (row * rw + col) * bypp
                    if bypp == 4:
                        px = int.from_bytes(data[off:off+4], "big" if big_endian else "little")
                    elif bypp == 2:
                        px = int.from_bytes(data[off:off+2], "big" if big_endian else "little")
                    else:
                        px = data[off]
                    r = ((px >> rsh) & rmax) * 255 // max(rmax, 1)
                    g = ((px >> gsh) & gmax) * 255 // max(gmax, 1)
                    b = ((px >> bsh) & bmax) * 255 // max(bmax, 1)
                    doff = ((y + row) * w + (x + col)) * 3
                    img[doff:doff+3] = bytes((r, g, b))
            got += rw * rh
    with open(out, "wb") as f:
        f.write(b"P6\n%d %d\n255\n" % (w, h))
        f.write(bytes(img))
    print(f"wrote {out} ({got} px)")

if __name__ == "__main__":
    main()
