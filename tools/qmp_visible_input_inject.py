#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

"""Inject a deterministic visible-input proof sequence via QMP."""

from __future__ import annotations

import json
import socket
import sys
import time
from pathlib import Path

DEBUG_LOG_PATH = Path("/home/jenning/open-nexus-OS/.cursor/debug-8cde1d.log")
DEBUG_SESSION_ID = "8cde1d"
DEBUG_RUN_ID = "visible-bootstrap-qmp"


def recv_json(sock: socket.socket) -> dict:
    buffer = bytearray()
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            raise RuntimeError("qmp socket closed")
        buffer.extend(chunk)
        while b"\n" in buffer:
            line, _, rest = buffer.partition(b"\n")
            buffer[:] = rest
            line = line.strip()
            if not line:
                continue
            return json.loads(line.decode("utf-8"))


def send_qmp(sock: socket.socket, payload: dict) -> dict:
    sock.sendall(json.dumps(payload).encode("utf-8") + b"\n")
    while True:
        reply = recv_json(sock)
        if "event" in reply:
            continue
        return reply


def send_input_events(
    sock: socket.socket,
    events: list[dict],
    *,
    console: int | None = 0,
    device: str | None = None,
) -> None:
    arguments: dict[str, object] = {"events": events}
    if console is not None:
        arguments["console"] = console
    if device is not None:
        arguments["device"] = device
    reply = send_qmp(
        sock,
        {
            "execute": "input-send-event",
            "arguments": arguments,
        },
    )
    if "error" in reply:
        raise RuntimeError(f"input-send-event failed: {reply['error']}")


# region agent log
def append_debug_log(hypothesis_id: str, location: str, message: str, data: dict) -> None:
    payload = {
        "sessionId": DEBUG_SESSION_ID,
        "runId": DEBUG_RUN_ID,
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": int(time.time() * 1000),
    }
    try:
        with DEBUG_LOG_PATH.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(payload, separators=(",", ":")) + "\n")
    except Exception:
        pass


# endregion agent log


def wait_for_socket(path: Path, timeout_s: float = 15.0) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for qmp socket: {path}")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: qmp_visible_input_inject.py <qmp-socket>", file=sys.stderr)
        return 2

    socket_path = Path(sys.argv[1])
    # region agent log
    append_debug_log(
        "H4",
        "tools/qmp_visible_input_inject.py:104",
        "injector process started",
        {"socket": str(socket_path)},
    )
    # endregion agent log
    wait_for_socket(socket_path)

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.connect(str(socket_path))
        greeting = recv_json(sock)
        if "QMP" not in greeting:
            raise RuntimeError(f"unexpected qmp greeting: {greeting}")
        reply = send_qmp(sock, {"execute": "qmp_capabilities"})
        if "error" in reply:
            raise RuntimeError(f"qmp_capabilities failed: {reply['error']}")
        # region agent log
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:108",
            "qmp capabilities ready",
            {"socket": str(socket_path), "profile": "visible-bootstrap"},
        )
        # endregion agent log

        # Give the guest time to finish scene setup and route service startup.
        time.sleep(8.0)

        # The proof scene starts with the cursor at (24,12). Move once to the
        # click target, click, and then type a key through the real path.
        send_input_events(
            sock,
            [
                {"type": "rel", "data": {"axis": "x", "value": -16}},
                {"type": "rel", "data": {"axis": "y", "value": 28}},
            ],
            console=None,
            device="video0",
        )
        # region agent log
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:126",
            "pointer injection sent",
            {"device": "video0", "dx": -16, "dy": 28, "button": "left"},
        )
        # endregion agent log
        time.sleep(0.10)
        send_input_events(
            sock,
            [{"type": "btn", "data": {"down": True, "button": "left"}}],
            console=None,
            device="video0",
        )
        time.sleep(0.05)
        send_input_events(
            sock,
            [{"type": "btn", "data": {"down": False, "button": "left"}}],
            console=None,
            device="video0",
        )
        time.sleep(0.25)
        send_input_events(
            sock,
            [
                {
                    "type": "key",
                    "data": {"down": True, "key": {"type": "qcode", "data": "a"}},
                },
            ],
        )
        time.sleep(0.05)
        send_input_events(
            sock,
            [
                {
                    "type": "key",
                    "data": {"down": False, "key": {"type": "qcode", "data": "a"}},
                },
            ],
        )
        # region agent log
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:156",
            "keyboard injection sent",
            {"console": 0, "key": "a", "sequence": ["down", "up"]},
        )
        # endregion agent log
        time.sleep(0.10)

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # pragma: no cover - script-side diagnostics only
        # region agent log
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:196",
            "injector failed",
            {"error": str(exc)},
        )
        # endregion agent log
        print(f"[error] qmp visible-input inject failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
