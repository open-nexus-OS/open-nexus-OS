#!/usr/bin/env python3
"""Click-storm the running guest over QMP — the structural-interaction load
proof for the app-host heap (TASK-0077C, ADR-0065).

`input-flood` floods pointer MOVES (hover: no relayout). Nothing in the tree
drove hundreds of STRUCTURAL interactions — the ones that rebuild a frame and
used to leak ~75 KiB each — so the app-host's memory over a long session was
proven on the host harness and by mechanism, never by a boot. This alternates
a click on the top-bar status pill (opens the Control Center) with a click on
the scrim (closes it): two structural frames per pair. The proof is read off
the `apphost: frame arena (… base=…)` samples: the base heap must not move.

Not yet wired into a lane (TASK-0145B P3b): QEMU accepts ONE QMP client and the
visible profile's own injector (`qmp_visible_input_inject.py`) holds it until the
ladder ends, so there is no window for a second client in that profile — and
without `QEMU_INPUT_AUTOINJECT=1` no socket exists at all. The lane needs a
profile whose injector IS this storm.

Usage: qmp_click_storm.py <qmp-socket> <pairs> [hz]
"""
import json, socket, sys, time

sock_path, pairs = sys.argv[1], int(sys.argv[2])
hz = float(sys.argv[3]) if len(sys.argv) > 3 else 3.0
W, H, MAX = 1280, 800, 32767
PILL, SCRIM = (1254, 18), (400, 600)   # status pill (29x28 at 1240,4); open-panel scrim

def abs_value(px, extent):
    return max(0, min(MAX, px * MAX // max(1, extent - 1)))

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
for _ in range(60):
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
        chunk = s.recv(65536)
        if not chunk:
            sys.exit("qmp peer closed")
        buf += chunk

def send(events):
    s.sendall(json.dumps({"execute": "input-send-event", "arguments": {"events": events}}).encode() + b"\n")
    recv()

recv()
s.sendall(json.dumps({"execute": "qmp_capabilities"}).encode() + b"\n")
recv()

period = 1.0 / hz
for n in range(pairs):
    for (x, y) in (PILL, SCRIM):
        send([{"type": "abs", "data": {"axis": "x", "value": abs_value(x, W)}},
              {"type": "abs", "data": {"axis": "y", "value": abs_value(y, H)}}])
        time.sleep(0.05)
        send([{"type": "btn", "data": {"down": True, "button": "left"}}])
        time.sleep(0.03)
        send([{"type": "btn", "data": {"down": False, "button": "left"}}])
        time.sleep(period)
print(f"STORM done: {pairs} open/close pairs = {2*pairs} structural interactions")
