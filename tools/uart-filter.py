#!/usr/bin/env python3
"""
Small helper to post-process `uart.log` / probe streams.

Features:
  * `--strip-escape` removes the `E`-prefixed probe encoding (alloc/probe lines).
  * `--grep foo --grep bar` keeps lines containing all provided substrings.
  * `--exclude noisy` drops lines containing the substrings.

Usage examples:
  $ tools/uart-filter.py --strip-escape uart.log | less
  $ tools/uart-filter.py --grep "init:" --exclude "svc-meta" uart.log
"""

from __future__ import annotations

import argparse
import sys
from typing import Iterable, Iterator, TextIO


def _decode_escape(line: str) -> str:
    """Strip the E-prefixed encoding emitted by the probe UART shim."""
    stripped = line.rstrip("\n")
    if not stripped or stripped[0] != "E":
        return line

    body = stripped
    # We only decode if every even index is the sentinel 'E'. Otherwise we may
    # be looking at a real log line that happens to contain 'E'.
    if not all(ch == "E" for ch in body[0::2]):
        return line

    decoded = "".join(body[idx] for idx in range(1, len(body), 2))
    if line.endswith("\n"):
        decoded += "\n"
    return decoded


def _should_keep(line: str, includes: Iterable[str], excludes: Iterable[str]) -> bool:
    for needle in includes:
        if needle not in line:
            return False
    for needle in excludes:
        if needle in line:
            return False
    return True


def _iter_lines(handle: TextIO, strip_escape: bool) -> Iterator[str]:
    for raw in handle:
        line = _decode_escape(raw) if strip_escape else raw
        yield line


def _iter_debug_stream(lines: Iterable[str]) -> Iterator[str]:
    import re

    pattern = re.compile(r"SYSCALL a7=([0-9a-fA-F]+)\s+a0=([0-9a-fA-F]+)")
    buffer: list[str] = []
    for line in lines:
        match = pattern.search(line)
        if not match:
            continue
        a7 = int(match.group(1), 16)
        if a7 != 0x10:
            continue
        a0 = int(match.group(2), 16) & 0xFF
        ch = chr(a0) if 0x20 <= a0 <= 0x7E or a0 in (0x0A, 0x0D, 0x09) else ""
        if a0 in (0x0A, 0x0D):
            if buffer:
                yield "".join(buffer) + "\n"
                buffer.clear()
            else:
                yield "\n"
        elif ch:
            buffer.append(ch)
    if buffer:
        yield "".join(buffer)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Filter/clean UART logs")
    parser.add_argument("path", nargs="?", help="Input file (defaults to stdin)")
    parser.add_argument(
        "--strip-escape",
        action="store_true",
        help="Decode probe lines prefixed with 'E' characters",
    )
    parser.add_argument(
        "--grep",
        action="append",
        default=[],
        help="Keep lines containing this substring (can be repeated, AND semantics)",
    )
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="Drop lines containing this substring (can be repeated)",
    )
    parser.add_argument(
        "--extract-debug-putc",
        action="store_true",
        help="Extract characters written via sys_debug_putc from kernel ECALL logs",
    )
    args = parser.parse_args(argv)

    handle: TextIO
    if args.path:
        handle = open(args.path, "r", encoding="utf-8", errors="replace")
    else:
        handle = sys.stdin

    try:
        lines = _iter_lines(handle, args.strip_escape)
        if args.extract_debug_putc:
            for decoded in _iter_debug_stream(lines):
                sys.stdout.write(decoded)
        else:
            for line in lines:
                if _should_keep(line, args.grep, args.exclude):
                    sys.stdout.write(line)
    finally:
        if handle is not sys.stdin:
            handle.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
