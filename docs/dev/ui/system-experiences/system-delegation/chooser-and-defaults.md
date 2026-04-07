<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Chooser And Defaults

This page covers how delegation targets are presented, ordered, and made default.

Primary task anchors:

- `tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md`
- `tasks/TASK-0126B-system-delegation-v1a-intent-actions-defaults-policy-host.md`
- `tasks/TRACK-SYSTEM-DELEGATION.md`

## Scope

- chooser ordering,
- per-action defaults,
- eligibility rules,
- and deterministic presentation of targets.

## Key posture

- ordering must be deterministic,
- defaults must be auditable,
- some defaults require stronger eligibility gates,
- and chooser UI belongs to SystemUI rather than target apps.

## Strong example

Default chat is not a simple app preference; it is a policy-sensitive system default with transfer and migration
expectations.
