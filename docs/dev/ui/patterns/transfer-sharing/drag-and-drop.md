<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Drag & Drop

Drag & drop should be:

- capability-based (no raw paths),
- deterministic (stable MIME/URI resolution),
- accessible (keyboard alternatives).

Drag and drop is TASK-0086's (moved there 2026-10-07): windowd routes the drag and never
draws its image; the payload rides clipboardd's one-shot transfer items (RFC-0094 Phase 4).
