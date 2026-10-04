#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Boot-protocol capture over usbfs (TASK-0328 U0): detach the stock HID driver from ONE
# interface, SET_PROTOCOL(boot) + SET_IDLE(0) exactly as our host stack will, read the
# interrupt-IN endpoint for a bounded time, print every report with its length and a
# millisecond stamp (a refused SET_IDLE is reported, not fatal), then SET_PROTOCOL(report) and hand the interface back to the stock
# driver. Nothing else is written. Usage (as root on the stock system):
#   boot_capture.py <bus> <devnum> <interface> <endpoint-hex> <max-packet> <seconds>
import ctypes, fcntl, os, struct, sys, time

USBDEVFS_CONTROL = 0xC0185500
USBDEVFS_BULK = 0xC0185502
USBDEVFS_RELEASEINTERFACE = 0x80045510
USBDEVFS_IOCTL = 0xC0105512
USBDEVFS_CONNECT = 0x5517
USBDEVFS_DISCONNECT_CLAIM = 0x8108551B


def control(fd, req_type, req, value, index, length, timeout_ms=1000):
    buf = ctypes.create_string_buffer(max(length, 1))
    arg = struct.pack("=BBHHHI4xQ", req_type, req, value, index, length, timeout_ms,
                      ctypes.addressof(buf))
    n = fcntl.ioctl(fd, USBDEVFS_CONTROL, bytearray(arg))
    return buf.raw[:length]


def interrupt_in(fd, ep, length, timeout_ms):
    buf = ctypes.create_string_buffer(length)
    arg = bytearray(struct.pack("=III4xQ", ep, length, timeout_ms, ctypes.addressof(buf)))
    try:
        n = fcntl.ioctl(fd, USBDEVFS_BULK, arg)
    except OSError as e:
        if e.errno == 110:  # ETIMEDOUT: no report in this window
            return None
        raise
    return buf.raw[:n]


def main():
    bus, dev, ifno, ep, mps, secs = sys.argv[1:7]
    ifno, ep, mps, secs = int(ifno), int(ep, 16), int(mps), float(secs)
    path = "/dev/bus/usb/%03d/%03d" % (int(bus), int(dev))
    fd = os.open(path, os.O_RDWR)
    claim = struct.pack("=II256s", ifno, 0, b"")
    fcntl.ioctl(fd, USBDEVFS_DISCONNECT_CLAIM, bytearray(claim))
    print("claimed %s interface %d (stock driver detached)" % (path, ifno))
    try:
        # SET_PROTOCOL(0 = boot), SET_IDLE(0 = report only on change), GET_PROTOCOL.
        control(fd, 0x21, 0x0B, 0, ifno, 0)
        try:
            control(fd, 0x21, 0x0A, 0, ifno, 0)
            print("SET_IDLE(0) accepted")
        except OSError as e:
            print("SET_IDLE(0) refused: %s (a STALL: optional for a mouse)" % e)
        proto = control(fd, 0xA1, 0x03, 0, ifno, 1)
        print("GET_PROTOCOL -> %s (0 = boot)" % proto.hex())
        t0 = time.monotonic()
        count = 0
        while time.monotonic() - t0 < secs:
            rep = interrupt_in(fd, ep, mps, 500)
            if rep is None:
                continue
            count += 1
            print("%8.1f ms len=%2d %s" % ((time.monotonic() - t0) * 1000, len(rep), rep.hex(" ")))
        print("reports: %d in %.0f s" % (count, secs))
    finally:
        try:
            control(fd, 0x21, 0x0B, 1, ifno, 0)  # back to report protocol
        except OSError as e:
            print("restore SET_PROTOCOL(report) failed: %s" % e)
        fcntl.ioctl(fd, USBDEVFS_RELEASEINTERFACE, bytearray(struct.pack("=I", ifno)))
        fcntl.ioctl(fd, USBDEVFS_IOCTL, bytearray(struct.pack("=iiQ", ifno, USBDEVFS_CONNECT, 0)))
        print("released interface %d, stock driver reattached" % ifno)
        os.close(fd)


if __name__ == "__main__":
    main()
