#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

"""Inject a deterministic visible-input proof sequence via QMP."""

from __future__ import annotations

import json
import os
import socket
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEBUG_LOG_PATH = Path("/home/jenning/open-nexus-OS/.cursor/debug-8cde1d.log")
DEBUG_SESSION_ID = "8cde1d"
DEBUG_RUN_ID = "visible-bootstrap-qmp"
DEFAULT_UART_LOG_PATH = REPO_ROOT / "uart.log"
DEFAULT_WAIT_MARKER = "windowd: present visible ok"
DEFAULT_WAIT_TIMEOUT_S = 60.0
QEMU_ABS_MAX = 32767
VISIBLE_ROUTE_WIDTH = 64
VISIBLE_ROUTE_HEIGHT = 48
VISIBLE_DISPLAY_WIDTH = 1280
VISIBLE_DISPLAY_HEIGHT = 800
HOVER_TARGET_ROUTE_X = 8
HOVER_TARGET_ROUTE_Y = 40
CURSOR_START_ROUTE_X = 24
CURSOR_START_ROUTE_Y = 12
REL_STEP_LIMIT = 256
POST_PRESENT_SETTLE_S = 0.10
POINTER_DOWN_HOLD_S = 0.25
KEY_DOWN_HOLD_S = 0.25
POINTER_RELEASE_SETTLE_S = 0.05
WHEEL_PULSE_SETTLE_S = 0.20


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
    console: int | None = None,
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
    if "error" not in reply:
        return
    if device is not None and reply["error"].get("class") == "DeviceNotFound":
        fallback_arguments = {"events": events}
        if console is not None:
            fallback_arguments["console"] = console
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:74",
            "pointer device fallback without explicit qmp device",
            {"requested_device": device, "error": reply["error"]},
        )
        fallback = send_qmp(
            sock,
            {
                "execute": "input-send-event",
                "arguments": fallback_arguments,
            },
        )
        if "error" not in fallback:
            return
        raise RuntimeError(f"input-send-event fallback failed: {fallback['error']}")
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


def wait_for_uart_marker(path: Path, marker: str, timeout_s: float) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            if path.exists() and marker in path.read_text(encoding="utf-8", errors="ignore"):
                return
        except OSError:
            pass
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for uart marker {marker!r} in {path}")


def route_cell_midpoint(route_coord: int, route_extent: int, display_extent: int) -> int:
    start = (route_coord * display_extent) // route_extent
    end = ((route_coord + 1) * display_extent + route_extent - 1) // route_extent
    end = max(end, start + 1)
    return (start + end - 1) // 2


def qemu_abs_value(display_coord: int, display_extent: int) -> int:
    if display_extent <= 1:
        return 0
    return (display_coord * QEMU_ABS_MAX) // (display_extent - 1)


def bounded_rel_steps(delta: int) -> list[int]:
    steps: list[int] = []
    remaining = delta
    while remaining != 0:
        if remaining > 0:
            step = min(remaining, REL_STEP_LIMIT)
        else:
            step = max(remaining, -REL_STEP_LIMIT)
        steps.append(step)
        remaining -= step
    return steps


def env_flag(name: str) -> bool:
    return os.environ.get(name, "0") == "1"


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

    touch_enabled = env_flag("NEXUS_PROFILE_INPUT_TOUCH")
    mouse_enabled = env_flag("NEXUS_PROFILE_INPUT_MOUSE")
    keyboard_enabled = env_flag("NEXUS_PROFILE_INPUT_KBD")
    session_mode = os.environ.get("QEMU_SESSION_MODE", "proof")
    uart_log_path = Path(os.environ.get("QEMU_UART_LOG_PATH", str(DEFAULT_UART_LOG_PATH)))
    wait_marker = os.environ.get("QEMU_INPUT_INJECT_WAIT_MARKER", DEFAULT_WAIT_MARKER)
    wait_timeout_s = float(
        os.environ.get("QEMU_INPUT_INJECT_WAIT_TIMEOUT_S", str(DEFAULT_WAIT_TIMEOUT_S))
    )
    proof_prefers_single_pointer_source = (
        session_mode == "proof" and touch_enabled and mouse_enabled
    )
    effective_touch_enabled = touch_enabled and not proof_prefers_single_pointer_source
    effective_mouse_enabled = mouse_enabled

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
            {
                "socket": str(socket_path),
                "profile": "visible-bootstrap",
                "uart_log_path": str(uart_log_path),
                "wait_marker": wait_marker,
                "wait_timeout_s": wait_timeout_s,
            },
        )
        # endregion agent log

        wait_for_uart_marker(uart_log_path, wait_marker, wait_timeout_s)
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:122",
            "guest visible-present marker reached",
            {"uart_log_path": str(uart_log_path), "wait_marker": wait_marker},
        )
        time.sleep(POST_PRESENT_SETTLE_S)

        # Drive the proof through the canonical display-space pipeline: compute
        # the physical midpoint of the hover target cell and inject whichever
        # pointer mode the proof lane exposed for this run.
        target_display_x = route_cell_midpoint(
            HOVER_TARGET_ROUTE_X, VISIBLE_ROUTE_WIDTH, VISIBLE_DISPLAY_WIDTH
        )
        target_display_y = route_cell_midpoint(
            HOVER_TARGET_ROUTE_Y, VISIBLE_ROUTE_HEIGHT, VISIBLE_DISPLAY_HEIGHT
        )
        start_display_x = route_cell_midpoint(
            CURSOR_START_ROUTE_X, VISIBLE_ROUTE_WIDTH, VISIBLE_DISPLAY_WIDTH
        )
        start_display_y = route_cell_midpoint(
            CURSOR_START_ROUTE_Y, VISIBLE_ROUTE_HEIGHT, VISIBLE_DISPLAY_HEIGHT
        )
        rel_x_steps = (
            bounded_rel_steps(target_display_x - start_display_x)
            if effective_mouse_enabled
            else []
        )
        rel_y_steps = (
            bounded_rel_steps(target_display_y - start_display_y)
            if effective_mouse_enabled
            else []
        )
        if effective_mouse_enabled:
            for idx in range(max(len(rel_x_steps), len(rel_y_steps))):
                rel_x = rel_x_steps[idx] if idx < len(rel_x_steps) else 0
                rel_y = rel_y_steps[idx] if idx < len(rel_y_steps) else 0
                if rel_x == 0 and rel_y == 0:
                    continue
                send_input_events(
                    sock,
                    [
                        {"type": "rel", "data": {"axis": "x", "value": rel_x}},
                        {"type": "rel", "data": {"axis": "y", "value": rel_y}},
                    ],
                    console=None,
                    device="video0",
                )
                time.sleep(0.05)
        if effective_touch_enabled:
            send_input_events(
                sock,
                [
                    {
                        "type": "abs",
                        "data": {"axis": "x", "value": qemu_abs_value(target_display_x, VISIBLE_DISPLAY_WIDTH)},
                    },
                    {
                        "type": "abs",
                        "data": {"axis": "y", "value": qemu_abs_value(target_display_y, VISIBLE_DISPLAY_HEIGHT)},
                    },
                ],
                console=None,
                device="video0",
            )
        # region agent log
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:126",
            "pointer injection sent",
            {
                "device": "video0",
                "touch_enabled": touch_enabled,
                "mouse_enabled": mouse_enabled,
                "effective_touch_enabled": effective_touch_enabled,
                "effective_mouse_enabled": effective_mouse_enabled,
                "keyboard_enabled": keyboard_enabled,
                "session_mode": session_mode,
                "proof_prefers_single_pointer_source": proof_prefers_single_pointer_source,
                "route_target": [HOVER_TARGET_ROUTE_X, HOVER_TARGET_ROUTE_Y],
                "display_start": [start_display_x, start_display_y],
                "display_target": [target_display_x, target_display_y],
                "rel_steps": list(zip(rel_x_steps, rel_y_steps, strict=False)),
                "qemu_abs_target": [
                    qemu_abs_value(target_display_x, VISIBLE_DISPLAY_WIDTH),
                    qemu_abs_value(target_display_y, VISIBLE_DISPLAY_HEIGHT),
                ],
                "button": "left",
            },
        )
        # endregion agent log
        time.sleep(POST_PRESENT_SETTLE_S)
        if effective_mouse_enabled or effective_touch_enabled:
            send_input_events(
                sock,
                [{"type": "btn", "data": {"down": True, "button": "left"}}],
                console=None,
                device="video0",
            )
            time.sleep(POINTER_DOWN_HOLD_S)
        if keyboard_enabled:
            send_input_events(
                sock,
                [
                    {
                        "type": "key",
                        "data": {"down": True, "key": {"type": "qcode", "data": "a"}},
                    },
                ],
            )
            time.sleep(KEY_DOWN_HOLD_S)
            send_input_events(
                sock,
                [
                    {
                        "type": "key",
                        "data": {"down": False, "key": {"type": "qcode", "data": "a"}},
                    },
                ],
            )
        if effective_mouse_enabled or effective_touch_enabled:
            time.sleep(POINTER_RELEASE_SETTLE_S)
            send_input_events(
                sock,
                [{"type": "btn", "data": {"down": False, "button": "left"}}],
                console=None,
                device="video0",
            )
        if effective_mouse_enabled:
            send_input_events(
                sock,
                [{"type": "btn", "data": {"down": True, "button": "wheel-up"}}],
                console=None,
                device="video0",
            )
            send_input_events(
                sock,
                [{"type": "btn", "data": {"down": False, "button": "wheel-up"}}],
                console=None,
                device="video0",
            )
            append_debug_log(
                "H4",
                "tools/qmp_visible_input_inject.py:176",
                "wheel injection sent",
                {"enabled": True, "button": "wheel-up", "sequence": ["down", "up"]},
            )
            time.sleep(WHEEL_PULSE_SETTLE_S)
        # region agent log
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py:156",
            "keyboard injection sent",
            {"enabled": keyboard_enabled, "key": "a", "sequence": ["down", "up"]},
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
