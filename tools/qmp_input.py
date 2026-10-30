#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""
CONTEXT: The visible-input injector's PRIMITIVES (TASK-0333 6f-3, ADR-0089) — how the host moves
the pointer, clicks, holds and drags, chords and types through QMP, and how it reads the guest's
answer from the UART. The choreography is not here: it is data (`proof-manifest/scenarios/*.toml`),
run by `qmp_scenario.py`.

POSITIONING: relative-mouse steps are NOT a position on the one-hart TCG guest — QEMU's USB boot
mouse merges queued reports with int8 clamping while the guest polls slowly, and inputd's
acceleration caps a delta at 256. So: home into the top-left corner in small steps (the display
clamp makes (0, 0) exact), travel in ≤40-px steps, CLICK, read back where app-host saw the tap
(`apphost: tap (x,y) …` / `… input tap miss at (x,y)`), correct from the observed position. A
press that never surfaced was merged away together with the travel before it: the position is
unknown then, so the runner homes again before it retries (`qmp_scenario.py`). An absolute
pointer (the tablet) is placed directly.

OWNERS: @ui @runtime  ·  STATUS: Functional  ·  API_STABILITY: Internal
"""
from __future__ import annotations

import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

# Relative travel: steps well under inputd's 256-px cap and QEMU's int8 report.
MODAL_STEP = 40
MODAL_STEP_PACE_S = 0.12
POINTER_DOWN_HOLD_S = 0.25
KEY_DOWN_HOLD_S = 0.25
SETTLE_S = 0.35
HOME_DRAIN_S = 0.6
TAP_TRACE_KEYS = ("apphost: tap (", "apphost: input tap miss at (")
# app-host's line per drag gesture: `apphost: drag (sx,sy)->(ex,ey) hit=Some(n)|None`.
DRAG_TRACE_KEY = "apphost: drag ("


@dataclass
class InputEnv:
    """What the primitives borrow from the injector (its socket and helpers)."""

    sock: object
    uart_log_path: Path
    display: tuple[int, int]
    mouse: bool
    touch: bool
    send_input_events: Callable
    append_debug_log: Callable
    qemu_abs_value: Callable


class InputDriver:
    def __init__(self, env: InputEnv) -> None:
        self.env = env
        self.pointer_at = [0, 0]

    # -- oracle -------------------------------------------------------------------
    def log(self, message: str, data: dict | None = None) -> None:
        self.env.append_debug_log("H4", "tools/qmp_input.py", message, data or {})

    def uart_text(self) -> str:
        try:
            return self.env.uart_log_path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return ""

    def uart_size(self) -> int:
        try:
            return self.env.uart_log_path.stat().st_size
        except OSError:
            return 0

    def uart_tail(self, since: int) -> str:
        try:
            with self.env.uart_log_path.open("rb") as fh:
                fh.seek(since)
                return fh.read().decode("utf-8", errors="replace")
        except OSError:
            return ""

    @staticmethod
    def parse_tap(tail: str) -> tuple[int, int] | None:
        for line in tail.splitlines():
            for key in TAP_TRACE_KEYS:
                if key in line:
                    try:
                        x_s, y_s = line.split(key, 1)[1].split(")", 1)[0].split(",")
                        return int(x_s), int(y_s)
                    except (ValueError, IndexError):
                        continue
        return None

    @staticmethod
    def parse_drag(tail: str) -> tuple[tuple[int, int], tuple[int, int], bool] | None:
        """The first drag trace in `tail`: (press, release, whether a box took the drag)."""
        for line in tail.splitlines():
            if DRAG_TRACE_KEY not in line:
                continue
            try:
                start, rest = line.split(DRAG_TRACE_KEY, 1)[1].split(")->(", 1)
                sx, sy = start.split(",")
                ex, ey = rest.split(")", 1)[0].split(",")
                return (int(sx), int(sy)), (int(ex), int(ey)), "hit=Some(" in rest
            except (ValueError, IndexError):
                continue
        return None

    def greeter_submit_box(self) -> tuple[int, int] | None:
        """The greeter's Submit circle from app-host's handler-box dump (its first tap
        miss prints every handler box): the one square box. Runtime truth, not a
        host-side guess of the greeter's geometry."""
        boxes = []
        for line in self.uart_text().splitlines():
            if "apphost: handler box id=" not in line or " w=" not in line:
                continue
            try:
                fields = dict(kv.split("=", 1) for kv in line.split("apphost: handler box ", 1)[1].split())
                w, h = int(fields["w"]), int(fields["h"])
                if w > 0 and w == h:
                    boxes.append((int(fields["x"]) + w // 2, int(fields["y"]) + h // 2))
            except (KeyError, ValueError, IndexError):
                continue
        return boxes[-1] if boxes else None

    # -- motion -------------------------------------------------------------------
    def rel(self, dx: int, dy: int) -> None:
        self.env.send_input_events(
            self.env.sock,
            [
                {"type": "rel", "data": {"axis": "x", "value": dx}},
                {"type": "rel", "data": {"axis": "y", "value": dy}},
            ],
            console=None,
        )
        time.sleep(MODAL_STEP_PACE_S)

    def home(self) -> None:
        if self.env.mouse:
            span = max(self.env.display) + 4 * MODAL_STEP
            for _ in range(span // MODAL_STEP + 1):
                self.rel(-MODAL_STEP, -MODAL_STEP)
            # Let the guest drain the homing reports before the first travel step.
            time.sleep(HOME_DRAIN_S)
        self.pointer_at = [0, 0]

    def travel(self, target_x: int, target_y: int) -> None:
        if self.env.mouse:
            while self.pointer_at[0] != target_x or self.pointer_at[1] != target_y:
                dx = max(-MODAL_STEP, min(MODAL_STEP, target_x - self.pointer_at[0]))
                dy = max(-MODAL_STEP, min(MODAL_STEP, target_y - self.pointer_at[1]))
                self.rel(dx, dy)
                self.pointer_at[0] += dx
                self.pointer_at[1] += dy
        if self.env.touch:
            w, h = self.env.display
            self.env.send_input_events(
                self.env.sock,
                [
                    {"type": "abs", "data": {"axis": "x", "value": self.env.qemu_abs_value(target_x, w)}},
                    {"type": "abs", "data": {"axis": "y", "value": self.env.qemu_abs_value(target_y, h)}},
                ],
                console=None,
            )
            self.pointer_at = [target_x, target_y]
        time.sleep(0.20)

    def press(self, button: str = "left") -> None:
        for down in (True, False):
            self.env.send_input_events(
                self.env.sock, [{"type": "btn", "data": {"down": down, "button": button}}], console=None
            )
            if down:
                time.sleep(POINTER_DOWN_HOLD_S)

    def button(self, down: bool, button: str = "left") -> None:
        self.env.send_input_events(
            self.env.sock, [{"type": "btn", "data": {"down": down, "button": button}}], console=None
        )

    def key(self, qcode: str) -> None:
        for down in (True, False):
            self.env.send_input_events(
                self.env.sock,
                [{"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": qcode}}}],
            )
            time.sleep(KEY_DOWN_HOLD_S)

    def click_observed(self, timeout_s: float = 10.0) -> tuple[int, int] | None:
        """Click at the current pointer and return where app-host saw the tap."""
        before = self.uart_size()
        self.press()
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            seen = self.parse_tap(self.uart_tail(before))
            if seen is not None:
                return seen
            time.sleep(0.1)
        return None

    def press_observed(self, timeout_s: float = 45.0) -> tuple[int, int] | None:
        """Press and HOLD at the current pointer; return where app-host saw the press (its own
        tap trace — consumed here, so no later click can take it for its own)."""
        before = self.uart_size()
        self.button(True)
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            seen = self.parse_tap(self.uart_tail(before))
            if seen is not None:
                return seen
            time.sleep(0.1)
        return None

    def drag_observed(
        self, since: int, timeout_s: float = 45.0
    ) -> tuple[tuple[int, int], tuple[int, int], bool] | None:
        """Wait for the drag trace app-host writes when the release ends the gesture."""
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            drag = self.parse_drag(self.uart_tail(since))
            if drag is not None:
                return drag
            time.sleep(0.2)
        return None

    def wait_settled(self, timeout_s: float = 25.0, since: int | None = None) -> None:
        """Wait until the tap's frame is on screen (`apphost: tap settled` — app-host's line
        for a traced POINTER tap, after its present or at once when nothing repaints): on the
        one-hart TCG lane a structural repaint takes seconds, and while the guest is that busy
        its USB polling stalls — QEMU then merges queued pointer reports and a button edge can
        vanish. Never a per-frame line: frames follow keystrokes too, and the guest writes no
        line per keystroke (the keystroke privacy rule)."""
        before = self.uart_size() if since is None else since
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            if "apphost: tap settled" in self.uart_tail(before):
                break
            time.sleep(0.2)
        time.sleep(0.5)

    def chord(self, qcodes: list[str]) -> None:
        """Hold the modifiers, tap the key, release in reverse — one chord."""
        *mods, key = qcodes
        for m in mods:
            self.env.send_input_events(
                self.env.sock, [{"type": "key", "data": {"down": True, "key": {"type": "qcode", "data": m}}}]
            )
            time.sleep(0.08)
        self.key(key)
        for m in reversed(mods):
            self.env.send_input_events(
                self.env.sock, [{"type": "key", "data": {"down": False, "key": {"type": "qcode", "data": m}}}]
            )
            time.sleep(0.08)

    def missing_after(
        self, markers: list[str], since: int, timeout_s: float, then: list[str] | None = None
    ) -> str | None:
        """Waits until `markers` appear as NEW lines after byte offset `since`, in any order
        (services write concurrently: only causality orders their lines), and `then` each after
        the LAST of them (a re-created surface's `presented` only counts after its `resized`);
        answers the first still missing when the bound runs out, `None` when all came."""
        deadline = time.monotonic() + timeout_s
        while True:
            tail = self.uart_tail(since)
            missing, last = None, 0
            for marker in markers:
                found = tail.find(marker)
                if found < 0:
                    missing = marker
                    break
                last = max(last, found + len(marker))
            if missing is None:
                missing = next((m for m in then or [] if tail.find(m, last) < 0), None)
            if missing is None:
                return None
            if time.monotonic() >= deadline:
                return missing
            time.sleep(0.2)
