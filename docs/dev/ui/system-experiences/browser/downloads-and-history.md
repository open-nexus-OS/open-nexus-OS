<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Downloads And History

Browser history, downloads, and similar shell data surfaces are shared system-facing UX, even when the rendering core is a
blessed web surface.

Primary anchors:

- `tasks/TASK-0205-webview-v1_2a-host-history-session-csp-cookies.md`
- `tasks/TASK-0206-webview-v1_2b-os-history-downloads-resume-csp-ui-recovery.md`

Posture:

- visible shell remains DSL-authored,
- queryable browser data should use the shared QuerySpec-shaped posture,
- and command flows such as open/reload/download remain explicit domain actions.
