<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# System Experiences

System experiences are OS-native, reusable user flows that apps should delegate to instead of re-implementing.

Use this category for:

- document picker and open-with,
- share/send style surfaces,
- browser shell/history-like system UX,
- capture/print/share style system flows,
- and other SystemUI-hosted, policy-mediated experiences.

Current entry points:

- `docs/dev/ui/system-experiences/system-delegation/README.md`
- `docs/dev/ui/system-experiences/document-access/README.md`
- `docs/dev/ui/system-experiences/browser/README.md`
- `docs/dev/ui/system-experiences/capture-and-share/README.md`
- `docs/dev/ui/system-experiences/doc-picker.md`
- `docs/dev/ui/blessed-surfaces/webview.md`
- `docs/dev/ui/system-experiences/capture-and-share/print.md`
- `docs/dev/ui/system-experiences/capture-and-share/print-preview.md`
- `docs/dev/ui/system-experiences/capture-and-share/screencap-share.md`

Related track:

- `tasks/TRACK-SYSTEM-DELEGATION.md`

Rule of thumb:

- if multiple apps should rely on the same safe system surface, document it here rather than in one app-specific page.
