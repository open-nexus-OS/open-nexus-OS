#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: The boot trace's OS contract (RFC-0107 Phase 2, TASK-0327B P2), run by
#          scripts/qemu-test.sh after every lane. For each boot of the run, the OS text
#          the block owner kept from the kernel's console ring must be the UART's text
#          after that boot's jump line, byte for byte. It is a prefix, since the console
#          runs on after the keeper's last write. A boot that stopped in the kernel before
#          the block owner ran (a fallback lane's trial boots) keeps no OS text, and that is
#          allowed for every boot but the run's last, which must reach `stage: platform`: the
#          keeper went on writing after the core services came up, not only once at its
#          start. The UART log keeps whole lines only (the launcher reads it line by line),
#          so a trace may run past the log's end by a partial last line, never by more.
# OWNERS:  @runtime @devx
# STATUS:  Functional
# API_STABILITY: Internal (the harness's)
# TEST_COVERAGE: every QEMU lane; mutations in TASK-0327B P2 (a changed byte, a missing boot,
#                a trace that stops early)
#
# Usage: trace_os_contract.py NX IMAGE UART_LOG SINCE_SEQ LOG_DIR
# Prints "<boots> <bytes>" and exits 0, or prints the reason (the first differing byte) and
# exits 1. Leaves each boot's OS text in LOG_DIR/trace-os-<seq>.txt.

import json
import subprocess
import sys


def fail(why):
    print(why)
    sys.exit(1)


def nx_run(nx, *args):
    out = subprocess.run([nx, "image", "trace", *args], capture_output=True, text=False)
    if out.returncode != 0:
        fail(f"nx image trace {' '.join(args)}: {out.stdout.decode(errors='replace').strip()}")
    return out.stdout


def main():
    if len(sys.argv) != 6:
        fail("usage: trace_os_contract.py NX IMAGE UART_LOG SINCE_SEQ LOG_DIR")
    nx, image, uart_path, since, log_dir = sys.argv[1:]
    uart = open(uart_path, "rb").read()
    meta = json.loads(nx_run(nx, "--image", image, "--since", since, "--json"))["data"]
    seqs = [boot["seq"] for boot in meta["boots"]]
    total = 0
    for i, seq in enumerate(seqs):
        out = f"{log_dir}/trace-os-{seq}.txt"
        nx_run(nx, "--image", image, "--seq", str(seq), "--os", "--out", out)
        text = open(out, "rb").read()
        mark = b"nxboot: trace slot=%d seq=%d" % ((seq - 1) % 8, seq)
        at = uart.find(mark)
        jump = uart.find(b"nxboot: jump ", at) if at >= 0 else -1
        if jump < 0:
            fail(f"boot {seq}: no jump line after its trace line in the UART log")
        start = uart.find(b"\n", jump) + 1
        if not text:
            # A boot that stopped in the kernel before the block owner ran (a fallback lane's
            # trial boots) leaves no OS text — RFC-0107's stated case. The run's last boot
            # must have reached it, though.
            if i == len(seqs) - 1:
                fail(f"boot {seq}: the trace keeps no OS text")
            continue
        shown = uart[start:start + len(text)]
        same = min(len(text), len(shown))
        diff = next((k for k in range(same) if text[k] != shown[k]), None)
        if diff is not None:
            lo = max(0, diff - 24)
            fail(
                f"boot {seq}: the kept OS text differs from the UART at byte {diff}: "
                f"trace {text[lo:diff + 40]!r} / uart {shown[lo:diff + 40]!r}"
            )
        if len(text) > len(shown) and b"\n" in text[len(shown):]:
            fail(
                f"boot {seq}: the trace holds {len(text) - len(shown)} bytes past the UART "
                "log's end — more than a partial last line"
            )
        if i == len(seqs) - 1 and b"stage: platform" not in text:
            fail(f"boot {seq}: the kept OS text ({len(text)} bytes) ends before `stage: platform`")
        total += len(text)
    print(len(seqs), total)


if __name__ == "__main__":
    main()
