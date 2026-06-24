// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Chat app (RFC-0067 P2.4) — chat as a real app that owns its content.
//! The message `model` (provider + pool) and the chat cell `view` live here, not
//! in the generic `nexus-virtual-list` widget or the desktop shell. windowd
//! consumes the provider's data; the scene-graph path uses the cell.
//! OWNERS: @ui
//! STATUS: Functional (model + cell view; windowd present wired via the existing
//! VirtualList chat path)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: model (provider/wrap/scroll) + view (themed bubble) host tests
//! ADR: docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md

// `no_std` for the non-test build so the OS compositor (windowd, no_std) can link
// the app's content directly; host tests keep `std`. `alloc` provides `Vec` in both.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod model;
pub mod view;

pub use model::{ChatMessage, ChatMessageProvider};
pub use view::ChatItemView;

/// Marker emitted once the app reports its content is ready.
pub const CHAT_APP_READY_MARKER: &str = "chat-app: content ready";

/// The default synthetic conversation windowd hosts today (5000 mixed-height
/// messages — the chat stress collection). The app owns this choice, not the
/// compositor.
pub fn default_provider() -> ChatMessageProvider {
    ChatMessageProvider::synthetic(5000, 40, 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_populated() {
        assert_eq!(default_provider().len(), 5000);
    }
}
