#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""
CONTEXT: the LIVE modal phase of the visible-input injector (TASK-0074 / ADR-0068),
split out of `qmp_visible_input_inject.py`: after the selftest ladder it logs in
(the greeter's Submit circle, read from app-host's own handler-box dump), opens the
Control Center, presses the power button, and runs the BOARD's sequence — Confirm →
system toast → its timeout → the alert once more → a background press (absorbed) →
ESC (imed delivers Escape to the focused surface without a text field). The last
marker it produces, `apphost: modal dismiss (reason=escape)`, is what the launcher's
early stop waits for (`QEMU_LADDER_ALSO_WAIT`, harness.toml).

POSITIONING: relative-mouse steps are NOT a position on the one-hart TCG guest —
QEMU's USB boot mouse merges queued reports with int8 clamping while the guest polls
slowly, and inputd's acceleration caps a delta at 256. So: home into the top-left
corner in small steps (the display clamp makes (0, 0) exact), travel in ≤40-px steps,
CLICK, read back where app-host saw the tap (`apphost: tap (x,y) …` / `… input tap
miss at (x,y)`), correct from the observed position; a press that never surfaced
(merged away) is pressed again. Targets are the SSOT constants asserted by
tests/dsl_apps_conformance/tests/shell_power_alert.rs (desktop session shell, 1280x800).

OWNERS: @ui @runtime  ·  STATUS: Functional  ·  API_STABILITY: Internal
"""
from __future__ import annotations

import os
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

# Display-space targets at 1280x800 in the DESKTOP session shell — SSOT:
# tests/dsl_apps_conformance/tests/shell_power_alert.rs (INJECT_*).
MODAL_CC_PILL = (1253, 18)
MODAL_POWER_BUTTON = (1169, 71)
MODAL_CONFIRM_BUTTON = (714, 428)
MODAL_BACKGROUND = (400, 400)
# TASK-0066 tiling phase — SSOT: tests/dsl_apps_conformance/tests/shell_tiling_lane.rs.
TILE_LAUNCHER_BUTTON = (30, 772)
TILE_LAUNCH_SETTINGS = (914, 662)

# Relative travel: steps well under inputd's 256-px cap and QEMU's int8 report.
MODAL_STEP = 40
MODAL_STEP_PACE_S = 0.12
POINTER_DOWN_HOLD_S = 0.25
KEY_DOWN_HOLD_S = 0.25
SETTLE_S = 0.35
HOME_DRAIN_S = 0.6
TAP_TRACE_KEYS = ("apphost: tap (", "apphost: input tap miss at (")


@dataclass
class ModalEnv:
    """What the phase borrows from the injector (its socket and helpers)."""

    sock: object
    uart_log_path: Path
    display: tuple[int, int]
    mouse: bool
    touch: bool
    rel_step_limit: int
    send_input_events: Callable
    wait_for_uart_marker: Callable
    append_debug_log: Callable
    qemu_abs_value: Callable


class ModalPhase:
    def __init__(self, env: ModalEnv) -> None:
        self.env = env
        self.pointer_at = [0, 0]

    # -- oracle -------------------------------------------------------------------
    def log(self, message: str, data: dict | None = None) -> None:
        self.env.append_debug_log("H4", "tools/qmp_inject_modal.py", message, data or {})

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

    def wait_settled(self, timeout_s: float = 25.0) -> None:
        """Wait for the present that follows a tap (`apphost: submitted …`): on the
        one-hart TCG lane a structural repaint takes seconds, and while the guest is
        that busy its USB polling stalls — QEMU then merges queued pointer reports and
        a button edge can vanish."""
        before = self.uart_size()
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            if "apphost: submitted" in self.uart_tail(before):
                break
            time.sleep(0.2)
        time.sleep(0.5)

    def go_click(self, target: tuple[int, int]) -> None:
        target_x, target_y = target
        self.travel(target_x, target_y)
        for attempt in range(2):
            # A press that does not surface for a while is almost always
            # DELAYED (the one-hart TCG guest is repainting), not lost: a
            # second press would land later on whatever is under the pointer
            # by then (it closed the Control Center once). Wait long, retry once.
            seen = self.click_observed(timeout_s=45.0)
            self.log("modal phase click observed", {"target": [target_x, target_y], "seen": seen, "attempt": attempt + 1})
            if seen is None:
                time.sleep(1.0)
                continue
            if abs(seen[0] - target_x) <= 6 and abs(seen[1] - target_y) <= 6:
                break
            # Re-anchor on the observed position and travel the rest.
            self.pointer_at = list(seen)
            self.travel(target_x, target_y)
        self.wait_settled()

    def wait_marker_after(self, marker: str, since: int, timeout_s: float) -> bool:
        """Wait for a NEW occurrence of `marker` written after byte offset `since`."""
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            if marker in self.uart_tail(since):
                return True
            time.sleep(0.2)
        self.log("marker missing", {"marker": marker, "since": since})
        return False

    def wait_marker(self, marker: str, timeout_s: float) -> bool:
        try:
            self.env.wait_for_uart_marker(self.env.uart_log_path, marker, timeout_s)
            return True
        except RuntimeError as exc:
            self.log("marker missing", {"marker": marker, "error": str(exc)})
            return False

    # -- the phase ----------------------------------------------------------------
    def wait_for_ladder_end(self) -> None:
        """Start AFTER the selftest ladder: its kernel IPC benchmarks measure round
        trips on the one hart, and a burst of mouse reports during them is contention
        the measurement would honestly report as a FAIL."""
        deadline = time.monotonic() + float(os.environ.get("QEMU_INPUT_INJECT_LADDER_END_WAIT_S", "240"))
        while time.monotonic() < deadline:
            text = self.uart_text()
            if "SELFTEST: ui resize ok" in text or "SELFTEST: Completed" in text:
                break
            time.sleep(1.0)
        self.log("modal phase starts after the ladder", {"ladder_end_seen": time.monotonic() < deadline})

    def login_if_needed(self) -> None:
        """The lanes boot into the greeter: Submit alone signs the dev session in."""
        session_marker = "windowd: session shell visible"
        if session_marker in self.uart_text():
            return
        self.home()
        if self.greeter_submit_box() is None:
            self.click_observed(timeout_s=3.0)  # a miss in the corner provokes the dump
            time.sleep(0.5)
        submit = self.greeter_submit_box()
        self.log("greeter submit box from the handler dump", {"submit": submit})
        if submit is not None:
            self.go_click(submit)
            if self.wait_marker(session_marker, 25.0):
                time.sleep(1.5)

    def run(self) -> None:
        timeout_s = float(os.environ.get("QEMU_INPUT_INJECT_MODAL_TIMEOUT_S", "12"))
        self.wait_for_ladder_end()
        self.login_if_needed()
        self.home()
        self.go_click(MODAL_CC_PILL)
        # Round A — the board's path: power → alert → Confirm → the system toast
        # → its timeout dismissal (the host's timer fires the layer's handler).
        self.go_click(MODAL_POWER_BUTTON)
        if not self.wait_marker("apphost: modal open (depth=1)", timeout_s):
            return
        self.wait_marker("windowd: win modal on (id=", 5.0)
        self.go_click(MODAL_CONFIRM_BUTTON)
        self.wait_marker("apphost: modal dismiss (reason=timeout)", 20.0)
        # Round B — the alert once more: a background press is absorbed by the
        # layer (the runtime's confinement), ESC closes it (imed → windowd →
        # app-host without a text field). The ESC dismissal is the phase's LAST
        # marker — the launcher's early stop waits for it.
        self.go_click(MODAL_POWER_BUTTON)
        if not self.wait_marker("apphost: modal open (depth=1)", timeout_s):
            return
        self.go_click(MODAL_BACKGROUND)
        self.key("esc")
        if self.wait_marker("apphost: modal dismiss (reason=escape)", timeout_s):
            self.log("modal phase complete (confirm → toast timeout → alert → background → ESC)")


    # -- tiling (TASK-0066 / ADR-0069) ------------------------------------------
    def chord(self, qcodes: list[str]) -> None:
        """Hold the modifiers, tap the key, release in reverse — one Super+Ctrl chord."""
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

    def run_tiling(self) -> None:
        """Open the launcher from the taskbar, launch the settings app from its footer, then
        tile that window with Super+Ctrl+← and bring it back with Super+Ctrl+↓: inputd's
        one-shot chord fact → windowd's one geometry path → `SELFTEST: ui v7 tile ok` (the
        phase's LAST marker, which the launcher's early stop waits for)."""
        timeout_s = float(os.environ.get("QEMU_INPUT_INJECT_MODAL_TIMEOUT_S", "12"))
        self.go_click(TILE_LAUNCHER_BUTTON)
        self.go_click(TILE_LAUNCH_SETTINGS)
        if not self.wait_marker("windowd: transition open", 40.0):
            return
        # The new window renders its first frame; let it settle before the chord.
        self.wait_settled(30.0)
        time.sleep(1.5)
        since = self.uart_size()
        self.chord(["ctrl", "meta_l", "left"])
        if not self.wait_marker("windowd: wm tile (zone=left-half", timeout_s):
            return
        # The tiled window re-creates its surface at the new size (destroy →
        # create); windowd parks focus meanwhile and would refuse a chord with
        # `no-focus`. Wait for the re-created window to take focus back.
        self.wait_marker_after("windowd: focus id=app", since, 40.0)
        self.wait_settled(20.0)
        time.sleep(1.0)
        self.chord(["ctrl", "meta_l", "down"])
        if self.wait_marker("SELFTEST: ui v7 tile ok", timeout_s):
            self.log("tiling phase complete (launcher → settings → Super+Ctrl+← → Super+Ctrl+↓)")


def run_modal_phase(env: ModalEnv) -> None:
    phase = ModalPhase(env)
    phase.run()
    phase.run_tiling()
