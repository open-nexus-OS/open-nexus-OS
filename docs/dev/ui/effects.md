<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Effects

UI effects (blur/shadows/etc.) are bounded and deterministic where required by goldens.

Default stance:

- follow `docs/dev/ui/performance-philosophy.md`,
- optimize unchanged-state and cached paths first,
- and make degrade behavior explicit instead of hidden.
