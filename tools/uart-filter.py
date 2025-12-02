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
    args = parser.parse_args(argv)

    handle: TextIO
    if args.path:
        handle = open(args.path, "r", encoding="utf-8", errors="replace")
    else:
        handle = sys.stdin

    try:
        for line in _iter_lines(handle, args.strip_escape):
            if _should_keep(line, args.grep, args.exclude):
                sys.stdout.write(line)
    finally:
        if handle is not sys.stdin:
            handle.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

