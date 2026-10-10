#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""Name the `KINIT: mm space pid=… sid=…` lines of a transcript.

The kernel prints the owner's service id (FNV-1a 64 of the service name); this reads the
names from the tree and prints the table. Usage: spaces.py <uart.log> [repo root]."""
import os
import re
import sys


def fnv(name):
    h = 0xCBF29CE484222325
    for b in name.encode():
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def names(root):
    found = {"init-lite", "nexus-init", "selftest-client", "app-host"}
    for d in ("source/services", "source/drivers", "source/apps", "source/init", "bundles"):
        base = os.path.join(root, d)
        for cur, dirs, _ in os.walk(base):
            if cur[len(base):].count(os.sep) < 3:
                found.update(dirs)
    return {fnv(n): n for n in found}


def main():
    log = sys.argv[1]
    root = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "../../../..")
    known = names(root)
    line_re = re.compile(r"mm space pid=(\d+) sid=0x([0-9a-f]+) rss=(\d+) dma=(\d+)")
    for line in open(log, errors="replace"):
        m = line_re.search(line)
        if m:
            sid = int(m[2], 16)
            name = known.get(sid, "(child) " + m[2] if sid else "(no service id)")
            print(f"pid={m[1]:>3}  {name:<28} rss={int(m[3]) / 1048576:8.2f} MiB  dma={int(m[4]) / 1048576:7.2f} MiB")


if __name__ == "__main__":
    main()
