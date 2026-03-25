// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Internal IPC wire helper module for netstackd facade
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod handles;
pub(crate) mod parse;
pub(crate) mod reply;
pub(crate) mod wire;
