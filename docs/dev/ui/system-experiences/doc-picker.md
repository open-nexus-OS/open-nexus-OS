<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Document Picker

The document picker is the UI surface for `content://` document access.

Goals:

- pathless workflows (streams, not filesystem paths),
- scoped grants and clear permission prompts,
- deterministic UX flows for tests.

## Query posture

The document picker is an early QuerySpec consumer.

Recommended posture:

- provider/source selection, MIME filters, search text, explicit ordering, and paging are represented as pure query state,
- picker UI updates that query state in reducers/composables,
- execution happens only through `contentd.query(...)` from effects/service adapters,
- and provider-specific presets may exist, but they should still compile to the shared query contract.

This keeps picker lists deterministic, testable, and ready for virtualization/lazy loading when provider result sets grow.
