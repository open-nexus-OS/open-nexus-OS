<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Confirmation And Consent

This page describes user-mediated confirmation and trust signaling for sensitive system-delegated actions.

Primary anchors:

- `tasks/TRACK-SYSTEM-DELEGATION.md`
- `tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md`

## Scope

- sensitive action confirmation,
- app identity and anti-phishing indicators,
- grant-aware send/post/location flows,
- and the split between low-risk inline actions and high-risk explicit confirmation.

## Posture

- no spoofable caller identity,
- explicit trust indicators for who is asking and what will happen,
- no confirmation spam for low-risk repeat actions,
- but strong user mediation for send/post/location and similar flows.
