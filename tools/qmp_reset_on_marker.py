#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Harness-side power-cycle actor for the `ota-fallback` lane
# (TASK-0289-B). A fault-fixture TRIAL boot parks in init on purpose (the
# honest brick); on real hardware a watchdog or the user pulls power.
# Under QEMU this watcher plays that role: it tails the uart log and
# issues ONE QMP system_reset per NEW occurrence of the marker, up to
# --count, then exits. Wait-loop doctrine: hard deadline, counted
# occurrences, exit on socket loss — never an unbounded babysitter.
#
# Usage: qmp_reset_on_marker.py <qmp-socket> <uart-log> <marker> <count> <timeout-s>

import json
import socket
import sys
import time


def qmp_reset(sock_path: str) -> bool:
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(5.0)
        s.connect(sock_path)
        f = s.makefile("rw", encoding="utf-8")
        f.readline()  # greeting
        f.write(json.dumps({"execute": "qmp_capabilities"}) + "\n")
        f.flush()
        f.readline()  # capabilities ack
        f.write(json.dumps({"execute": "system_reset"}) + "\n")
        f.flush()
        f.readline()  # reset ack
        s.close()
        return True
    except OSError as err:
        print(f"[qmp-reset] socket error: {err}", file=sys.stderr)
        return False


def main() -> int:
    sock_path, uart_path, marker, count_s, timeout_s = sys.argv[1:6]
    count = int(count_s)
    deadline = time.monotonic() + float(timeout_s)
    needle = marker.encode()
    fired = 0
    while time.monotonic() < deadline and fired < count:
        try:
            with open(uart_path, "rb") as f:
                seen = f.read().count(needle)
        except OSError:
            seen = 0
        if seen > fired:
            # One reset per loop turn even if several occurrences landed —
            # a reset consumes exactly one park, the next park re-arms us.
            print(f"[qmp-reset] marker occurrence {fired + 1}/{count} -> system_reset")
            sys.stdout.flush()
            if not qmp_reset(sock_path):
                return 1
            fired += 1
            time.sleep(1.0)
        else:
            time.sleep(0.3)
    if fired < count:
        print(f"[qmp-reset] deadline with {fired}/{count} resets fired", file=sys.stderr)
        return 1
    print(f"[qmp-reset] done ({fired} resets)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
