<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Text Editor (App)

This page documents the Text Editor app behavior and reusable patterns:

- autosave and recovery (bounded, deterministic in tests),
- print integration,
- file open/save via `content://` streams.

## Example (illustrative)

```nx
// On a bounded timer (injected clock in tests):
emit(TextEvent::AutosaveRequested)
```
