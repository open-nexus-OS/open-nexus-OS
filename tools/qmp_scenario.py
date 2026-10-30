#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""
CONTEXT: The visible lanes' scenario RUNNER (TASK-0333 6f-3, ADR-0089). A lane's choreography is
data — `proof-manifest/scenarios/<name>.toml`, validated by `nexus-proof-manifest scenario`
(every literal a declared marker, every target named, every action acknowledged) and handed over
as JSON. The runner only executes it:

  * nothing is injected before the guest's `start_after` line (the guest says when its timing
    gates are behind it — the host no longer guesses);
  * a step = an optional declared pause (`settle_s`), ONE action, then every `expect` marker as a
    NEW line after the action, in any order (services write concurrently: a close's `app window
    closed` and the app's `persist saved` have no order between them), and every `then` marker
    after the LAST `expect` line (a re-created surface's first frame only counts after its
    resize), within `timeout_s`; a click is also acknowledged by its own
    readback — a HIT on the point (`apphost: tap (x,y) hit=Some(…)`); a miss there means the
    surface under it is not laid out yet, so it waits and presses again — and a drag by its drag
    trace;
  * a `wait` step has no action of its own: what it awaits was caused by the step before it, so
    it counts from THAT step's action — counted from its own start it missed the restore line
    that came while the relaunch before it was still being acknowledged (2026-10-29);
  * a step whose acknowledgement does not come is retried (`retry`, with `retry_with` actions or
    the action again) — and otherwise ENDS the run with its name and the missing marker
    (`[scenario] FAIL step=… missing=…`, the hypothesis log): no step's outcome is ignored, no
    later step runs blind on a screen the failed one left behind;
  * a click whose press never surfaced was merged away with the travel before it (QEMU's USB
    mouse under a busy one-hart guest): the pointer's position is unknown, so it homes before
    it retries — the off-target press of 2026-10-29 (the clock instead of the search) came from
    retrying from an assumed position.

Every step's duration goes to the hypothesis log (`scenario step ok`): the lanes measure their
own choreography.

OWNERS: @runtime @ui  ·  STATUS: Functional  ·  API_STABILITY: Internal
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

from qmp_input import InputDriver, InputEnv

REPO_ROOT = Path(__file__).resolve().parents[1]
# The guest's window opens after its ladder; the bound only ends a dead run.
START_TIMEOUT_S = 900.0
# A click's presses: a lost press homes and presses again, an off-target press corrects, a press
# that MISSED on the point (the surface under it not laid out yet) waits and presses again.
CLICK_ATTEMPTS = 10
CLICK_TOLERANCE_PX = 6
MISS_SETTLE_S = 1.5
TAP_KEY = "apphost: tap ("
MISS_KEY = "apphost: input tap miss at ("
SESSION_SHELL_MARKER = "windowd: session shell visible"


def load_scenario(name: str) -> dict:
    """The validated scenario as JSON, from the one validator (`nexus-proof-manifest`)."""
    cli = os.environ.get("NEXUS_PROOF_MANIFEST_BIN") or str(
        REPO_ROOT / "target/debug/nexus-proof-manifest"
    )
    manifest = os.environ.get("NEXUS_PROOF_MANIFEST_PATH") or str(
        REPO_ROOT / "source/apps/selftest-client/proof-manifest/manifest.toml"
    )
    out = subprocess.run(
        [cli, "scenario", f"--scenario={name}", f"--manifest={manifest}"],
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        raise RuntimeError(f"scenario `{name}` refused: {out.stderr.strip()}")
    return json.loads(out.stdout)


class ScenarioRunner(InputDriver):
    def __init__(self, env: InputEnv) -> None:
        super().__init__(env)
        self.failure: dict | None = None

    # -- the run --------------------------------------------------------------------------------
    def run(self, scenario: dict) -> bool:
        name = scenario["name"]
        t0 = time.monotonic()
        missing = self.missing_after([scenario["start_after"]], 0, START_TIMEOUT_S)
        if missing is not None:
            return self.fail("start", missing, 1)
        self.log("scenario starts", {"scenario": name, "waited_s": round(time.monotonic() - t0, 1)})
        self.home()
        previous_since = self.uart_size()
        for step in scenario["steps"]:
            started = time.monotonic()
            missing, attempts, previous_since = self.step(step, previous_since)
            if missing is not None:
                return self.fail(step["name"], missing, attempts)
            self.log(
                "scenario step ok",
                {"step": step["name"], "s": round(time.monotonic() - started, 2), "attempts": attempts},
            )
        self.log("scenario complete", {"scenario": name, "s": round(time.monotonic() - t0, 1)})
        return True

    def fail(self, step: str, missing: str, attempts: int) -> bool:
        self.failure = {"step": step, "missing": missing, "attempts": attempts}
        self.log("scenario step failed", self.failure)
        print(f"[scenario] FAIL step={step!r} missing={missing!r} attempts={attempts}", file=sys.stderr)
        return False

    def step(self, step: dict, previous_since: int) -> tuple[str | None, int, int]:
        """One step: (the first missing acknowledgement or None, the attempts it took, the offset
        its acknowledgement counted from). A step that only waits counts from `previous_since`,
        the action of the step before it."""
        if step["settle_s"] > 0:
            time.sleep(step["settle_s"])
        attempts = 1 + step["retry"]
        missing: str | None = None
        since = previous_since
        for attempt in range(attempts):
            actions = [step["action"]] if attempt == 0 or not step["retry_with"] else step["retry_with"]
            if any(action["kind"] != "wait" for action in actions):
                since = self.uart_size()
            done, anywhere = self.act_all(actions)
            if done is not None:
                # The action's own readback failed: nothing it should have caused will come.
                missing = done
            else:
                missing = self.missing_after(
                    step["expect"], 0 if anywhere else since, step["timeout_s"], step["then"]
                )
            if missing is None:
                return None, attempt + 1, since
            self.log("scenario step retry", {"step": step["name"], "missing": missing, "attempt": attempt + 1})
        return missing, attempts, since

    def act_all(self, actions: list[dict]) -> tuple[str | None, bool]:
        """Performs `actions`; answers (the readback that failed or None, whether the expected
        state already held before them — a login that found the session up)."""
        anywhere = False
        for action in actions:
            kind = action["kind"]
            if kind == "login":
                result = self.login()
                if result == "already":
                    anywhere = True
                elif result is not None:
                    return result, False
            elif kind == "move":
                self.travel(*action["at"])
            elif kind == "click":
                if not self.click_at(tuple(action["at"])):
                    return f"tap at {action['target']} {tuple(action['at'])}", False
            elif kind == "chord":
                self.chord(action["keys"])
            elif kind == "key":
                self.key(action["key"])
            elif kind == "type":
                for letter in action["text"]:
                    self.key(letter)
            elif kind == "drag":
                if not self.drag(tuple(action["from_at"]), tuple(action["to_at"])):
                    return f"drag {action['from']} -> {action['to']}", False
            elif kind != "wait":
                return f"unknown action {kind}", False
        return None, anywhere

    # -- the actions ----------------------------------------------------------------------------
    def click_at(self, target: tuple[int, int]) -> bool:
        """Travel and press until app-host reports a HIT on the point (`apphost: tap (x,y)
        hit=Some(…)`). A miss there (`input tap miss at`, `hit=None`) means the surface under the
        point is not laid out yet — the session shell 5 s after its login (2026-10-29): wait and
        press again; only a hit acknowledges the click."""
        tx, ty = target
        self.travel(tx, ty)
        for attempt in range(CLICK_ATTEMPTS):
            since = self.uart_size()
            seen = self.press_readback(timeout_s=45.0)
            self.log("scenario click", {"target": [tx, ty], "seen": seen, "attempt": attempt + 1})
            if seen is None:
                # Never surfaced: merged away with the travel before it — the position is unknown.
                self.home()
                self.travel(tx, ty)
                continue
            x, y, hit = seen
            on_point = abs(x - tx) <= CLICK_TOLERANCE_PX and abs(y - ty) <= CLICK_TOLERANCE_PX
            if on_point and hit:
                self.wait_settled(since=since)
                return True
            if on_point:
                time.sleep(MISS_SETTLE_S)
                continue
            # Off the point: re-anchor on what the guest saw and travel the rest.
            self.pointer_at = [x, y]
            if hit:
                self.wait_settled(since=since)
            self.travel(tx, ty)
        return False

    def press_readback(self, timeout_s: float) -> tuple[int, int, bool] | None:
        """Click at the current pointer; where app-host saw it and whether it hit a handler."""
        before = self.uart_size()
        self.press()
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            for line in self.uart_tail(before).splitlines():
                for key in (TAP_KEY, MISS_KEY):
                    if key not in line:
                        continue
                    try:
                        x_s, y_s = line.split(key, 1)[1].split(")", 1)[0].split(",")
                        return int(x_s), int(y_s), key == TAP_KEY and " hit=Some(" in line
                    except (ValueError, IndexError):
                        continue
            time.sleep(0.1)
        return None

    def login(self) -> str | None:
        """The greeter's Submit circle, read from app-host's handler dump. `"already"` when the
        session shell was up before; a readback failure's name otherwise; `None` once pressed."""
        if SESSION_SHELL_MARKER in self.uart_text():
            return "already"
        if self.greeter_submit_box() is None:
            self.home()
            self.click_observed(timeout_s=3.0)  # a miss in the corner provokes the handler dump
            time.sleep(0.5)
        submit = self.greeter_submit_box()
        self.log("scenario greeter submit box", {"submit": submit})
        if submit is None:
            return "greeter submit box (no handler dump)"
        return None if self.click_at(submit) else f"tap at greeter submit {submit}"

    def drag(self, press: tuple[int, int], release: tuple[int, int]) -> bool:
        """Press at `press` (its own tap trace), travel with the button held, release at
        `release`: the drag trace with a box that took it acknowledges the gesture. Each edge waits
        for its OWN event — an edge injected before the previous one surfaced merges with it."""
        for attempt in range(2):
            self.travel(*press)
            seen = self.press_observed()
            self.log("scenario drag press", {"target": list(press), "seen": seen, "attempt": attempt + 1})
            if seen is not None and abs(seen[0] - press[0]) <= CLICK_TOLERANCE_PX and abs(seen[1] - press[1]) <= CLICK_TOLERANCE_PX:
                break
            # Off target or never surfaced: let go in place (a press without motion is a tap on
            # the scrim, which does nothing) and start over from a known position.
            self.button(False)
            self.wait_settled()
            if seen is not None:
                self.pointer_at = list(seen)
            else:
                self.home()
        else:
            return False
        self.travel(*release)
        time.sleep(0.35)
        since = self.uart_size()
        self.button(False)
        drag = self.drag_observed(since)
        self.log("scenario drag", {"target": [list(press), list(release)], "seen": drag})
        if drag is None or not drag[2]:
            return False
        self.pointer_at = list(drag[1])
        return True


def run_named(env: InputEnv, name: str) -> bool:
    """Loads, validates and runs scenario `name`; `False` with the reason in the hypothesis log."""
    runner = ScenarioRunner(env)
    try:
        scenario = load_scenario(name)
    except (RuntimeError, OSError, json.JSONDecodeError) as exc:
        runner.log("scenario refused", {"scenario": name, "error": str(exc)})
        print(f"[scenario] REFUSED {name}: {exc}", file=sys.stderr)
        return False
    return runner.run(scenario)
