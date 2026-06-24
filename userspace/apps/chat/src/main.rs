// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: chat app entry — reports its content is ready. The app owns the
//! message model; windowd hosts the rendered surface (RFC-0067 P2.4).
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (thin entry; logic host-tested in `chat_app` lib)
//! ADR: docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md

#![forbid(unsafe_code)]

fn main() {
    // The app owns its content (the message model); windowd composites the chat
    // window from this provider's data.
    let provider = chat_app::default_provider();
    debug_assert!(!provider.is_empty());
    println!("{}", chat_app::CHAT_APP_READY_MARKER);
}
