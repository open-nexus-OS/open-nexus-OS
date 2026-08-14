---
title: TASK-0037 OTA A/B v2b: real boot slot via bootargs/OpenSBI (Superseded 2026-08-14 by TASK-0289)
status: Superseded
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0036
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Depends-on: tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
---

## Context

> **SUPERSEDED (2026-08-14, sub-80 roadmap).** This was a 33-line placeholder holding the
> "real boot chain" ambition. TASK-0289 (boot trust floor v1: verified boot anchors, monotonic
> rollback indices, measured-boot handoff — created 2026-04-13, broader and newer) owns that
> ground now; 0037's one unique nugget — bootargs select the booted slot — folds into 0289's
> scope. Executing 0037 standalone would touch `source/kernel/**` + OpenSBI (approval zones)
> for a slice 0289 must redo anyway. Do not execute; see TASK-0289.

OTA A/B v2 wants an unambiguous “booted slot” determined at boot time. The prompt proposes
bootargs via OpenSBI/SBI handoff. With **kernel unchanged** and without an owned boot chain path,
this cannot be proven today.

This task exists to prevent drift: it documents the real boot integration work as a separate,
explicitly blocked deliverable.

## Goal

Once unblocked, prove:

- the selected slot is passed via bootargs at boot time (A/B),
- the OS reads it during early init and uses it to mount/select the correct system set,
- rollback scheduling actually affects the *next* real boot, not just a soft simulation.

## Red flags / decision points

- **RED**: blocked until boot chain integration exists (bootloader/OpenSBI/firmware handoff path).
