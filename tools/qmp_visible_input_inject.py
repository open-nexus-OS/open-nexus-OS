#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""
CONTEXT: The visible-input injector's ENTRY (TASK-0333 6f-3, ADR-0089): QEMU's launcher starts it
on the lanes whose profile enables injection (`QEMU_INPUT_AUTOINJECT=1`); it opens the QMP socket
and runs the lane's SCENARIO (`QEMU_INPUT_SCENARIO`, declared by the profile in
`proof-manifest/profiles/harness.toml` or by `scripts/input-flood-lane.sh`). What it presses and
what the guest must answer is data (`proof-manifest/scenarios/*.toml`), run by `qmp_scenario.py`
over the primitives in `qmp_input.py`. A lane without a scenario gets no input at all. The
imperative choreography this file held — a dead-reckoned pointer walk, tablet-only sidebar and
click steps that failed silently on every USB lane, waits on incidental markers — is gone.

Exit status: 0 when the scenario ran to its end (or there was none), 1 when a step's
acknowledgement never came (the step and the missing marker are in the hypothesis log).

OWNERS: @runtime @ui  ·  STATUS: Functional  ·  API_STABILITY: Internal
"""
from __future__ import annotations

import json
import os
import socket
import sys
import time
from pathlib import Path

# A sibling module import would write `tools/__pycache__`, which breaks the
# cargo workspace glob — never write bytecode from the harness.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import qmp_scenario  # noqa: E402
from qmp_input import InputEnv  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[1]
LOG_DIR = Path(os.environ.get("LOG_DIR", str(REPO_ROOT / "build/logs/manual-inject")))
HYPOTHESIS_LOG = LOG_DIR / "hypothesis.json"
RUN_ID = os.environ.get("RUN_ID", "visible-bootstrap-qmp")
DEFAULT_UART_LOG_PATH = Path(os.environ.get("UART_LOG", str(LOG_DIR / "uart.log")))
QEMU_ABS_MAX = 32767
VISIBLE_DISPLAY_WIDTH = 1280
VISIBLE_DISPLAY_HEIGHT = 800


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
        "runId": RUN_ID,
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": int(time.time() * 1000),
    }
    try:
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        with HYPOTHESIS_LOG.open("a", encoding="utf-8") as handle:
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


def qemu_abs_value(display_coord: int, display_extent: int) -> int:
    if display_extent <= 1:
        return 0
    return (display_coord * QEMU_ABS_MAX) // (display_extent - 1)


def env_flag(name: str) -> bool:
    return os.environ.get(name, "0") == "1"


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: qmp_visible_input_inject.py <qmp-socket>", file=sys.stderr)
        return 2
    socket_path = Path(sys.argv[1])
    scenario = os.environ.get("QEMU_INPUT_SCENARIO", "")
    append_debug_log(
        "H4",
        "tools/qmp_visible_input_inject.py",
        "injector process started",
        {"socket": str(socket_path), "scenario": scenario or None},
    )
    if not scenario:
        return 0
    wait_for_socket(socket_path)
    touch_enabled = env_flag("NEXUS_PROFILE_INPUT_TOUCH")
    mouse_enabled = env_flag("NEXUS_PROFILE_INPUT_MOUSE")
    # A proof boot drives ONE pointer source: the mouse when the profile names both.
    session_mode = os.environ.get("QEMU_SESSION_MODE", "proof")
    touch_enabled = touch_enabled and not (session_mode == "proof" and mouse_enabled)
    uart_log_path = Path(os.environ.get("QEMU_UART_LOG_PATH", str(DEFAULT_UART_LOG_PATH)))
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.connect(str(socket_path))
        greeting = recv_json(sock)
        if "QMP" not in greeting:
            raise RuntimeError(f"unexpected qmp greeting: {greeting}")
        reply = send_qmp(sock, {"execute": "qmp_capabilities"})
        if "error" in reply:
            raise RuntimeError(f"qmp_capabilities failed: {reply['error']}")
        env = InputEnv(
            sock=sock,
            uart_log_path=uart_log_path,
            display=(VISIBLE_DISPLAY_WIDTH, VISIBLE_DISPLAY_HEIGHT),
            mouse=mouse_enabled,
            touch=touch_enabled,
            send_input_events=send_input_events,
            append_debug_log=append_debug_log,
            qemu_abs_value=qemu_abs_value,
        )
        ok = qmp_scenario.run_named(env, scenario)
    return 0 if ok else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # pragma: no cover - script-side diagnostics only
        append_debug_log(
            "H4",
            "tools/qmp_visible_input_inject.py",
            "injector failed",
            {"error": str(exc)},
        )
        print(f"[error] qmp visible-input inject failed: {exc}", file=sys.stderr)
        raise SystemExit(1)
