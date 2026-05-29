// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Command buffer contract tests (encoding, validation, commit).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! TEST_SCOPE:
//!   - Command buffer creation and commit
//!   - Render pass validation (extent, attachments)
//!   - Command limit enforcement (MAX_COMMANDS, MAX_FRAGMENT_BYTES)
//!   - Sealed buffer immutability after commit
//!
//! ADR: docs/adr/0031-three-layer-animation-architecture.md
