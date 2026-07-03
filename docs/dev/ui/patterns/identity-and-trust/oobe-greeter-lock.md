<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# OOBE / Greeter / Lock Screen

> **Implementation status (2026-07-02, TASK-0065B).** The greeter v1 is live:
> blurred-wallpaper login window with a single avatar, click-to-login, session
> shell handoff, and authority-side launch gating — see
> `docs/dev/ui/shell/session.md` for the shipped contract (sessiond authority,
> SystemUI greeter manifest, windowd renderer). OOBE, credential auth, and the
> lock screen remain design-stage; `SessionState::Locked` and `OP_LOCK` are
> reserved seams for the lock flow below.

This document covers the shared UI flows for:

- out-of-box experience (OOBE),
- account selection/creation,
- lock screen and unlock prompts.

Goals:

- consistent visual language across tablet and desktop,
- secure-by-default interaction patterns (no spoofable identity),
- deterministic selftest markers for critical flows.
