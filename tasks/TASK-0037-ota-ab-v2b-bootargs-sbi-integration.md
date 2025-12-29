---
title: TASK-0037 OTA A/B v2b: real boot slot via bootargs/OpenSBI (blocked, requires boot chain support)
status: Blocked
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Depends-on: tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
---

## Context

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
