#!/usr/bin/env python3
"""Pointer-flood the running guest over QMP — the input-chain load proof.

TASK-0054C P2-g. Every other lane drives input politely: a few moves, a click,
a key. Nothing drove it HARD, and that is the gap a real user found by dragging
the mouse in the greeter: inputd allocated a `Vec` per HID batch on a heap that
never frees, exhausted 384 KiB in under twenty seconds, died, and took the whole
input chain with it (`hidrawd: tx hz=0` while events kept arriving) — so the
login could never be clicked and the boot sat on the greeter forever.

This floods absolute pointer moves at the rate a real drag produces in QEMU
(~900/s; the reported run logged `hidrawd: ev hz=1088`). The proof is what does
NOT happen: no `alloc-fail`, no `alloc_error`, and `tx hz` keeps tracking
`rx hz` to the end. Run it against a live QMP socket; `just input-flood` wires
the boot, the flood and the assertions together.
"""
import json, socket, sys, time

sock_path, seconds, rate = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
MAX = 32767

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
for attempt in range(60):
    try:
        s.connect(sock_path); break
    except OSError:
        time.sleep(1.0)
else:
    sys.exit("qmp socket never appeared")

buf = bytearray()
def recv():
    global buf
    while True:
        i = buf.find(b"\n")
        if i >= 0:
            line, buf = buf[:i], buf[i+1:]
            if line.strip():
                return json.loads(line)
        buf += s.recv(65536)

recv()                                   # greeting
s.sendall(json.dumps({"execute": "qmp_capabilities"}).encode() + b"\n")
recv()

sent = 0
t_end = time.time() + seconds
period = 1.0 / rate
x = y = 0
while time.time() < t_end:
    x = (x + 257) % MAX
    y = (y + 131) % MAX
    ev = [{"type": "abs", "data": {"axis": "x", "value": x}},
          {"type": "abs", "data": {"axis": "y", "value": y}}]
    s.sendall(json.dumps({"execute": "input-send-event",
                          "arguments": {"events": ev}}).encode() + b"\n")
    recv()
    sent += 1
    time.sleep(period)
print(f"FLOOD done: {sent} pointer moves over {seconds:.0f}s")
