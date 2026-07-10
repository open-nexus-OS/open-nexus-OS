// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Refresher` — the design-system pull-to-refresh wrapper (handoff
//! `Refresher`, ArkUI `SwipeRefresher` counterpart). This is the VIEW: it
//! composes the reveal zone (a spinner whose visible height follows the pull
//! `progress`) above the wrapped content. The PULL PHYSICS (drag tracking,
//! threshold, calling `onRefresh`) is the scroll runtime's job — the view
//! renders any (progress, refreshing) pair deterministically. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{Align, FxPx, LayoutNode};
use nexus_theme_tokens::Tokens;
use nexus_widget_panel::Panel;
use nexus_widget_spinner::Spinner;

/// Full reveal height (the spinner zone when the pull crosses the threshold).
const REVEAL_H: i32 = 48;

/// A pull-to-refresh wrapper.
#[derive(Debug, Clone)]
pub struct Refresher {
    /// Pull progress 0–100 (0 = resting; 100 = threshold reached).
    progress: u32,
    /// Whether a refresh is running (spinner fully revealed + animating).
    refreshing: bool,
    /// Spinner animation phase while refreshing (motion-system driven).
    phase: usize,
    content: Option<LayoutNode>,
    id: Option<&'static str>,
}

impl Default for Refresher {
    fn default() -> Self {
        Self { progress: 0, refreshing: false, phase: 0, content: None, id: None }
    }
}

impl Refresher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pull progress 0–100 (scroll runtime supplies it during the drag).
    pub fn progress(mut self, progress: u32) -> Self {
        self.progress = progress.min(100);
        self
    }

    /// Refresh in flight: the reveal stays open and the spinner animates.
    pub fn refreshing(mut self, on: bool) -> Self {
        self.refreshing = on;
        self
    }

    /// Spinner phase while refreshing (motion-system driven).
    pub fn phase(mut self, phase: usize) -> Self {
        self.phase = phase;
        self
    }

    /// The wrapped scrollable content.
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Visible reveal height for the current state.
    fn reveal_height(&self) -> i32 {
        if self.refreshing {
            REVEAL_H
        } else {
            REVEAL_H * self.progress as i32 / 100
        }
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let reveal_h = self.reveal_height();
        let mut root = Panel::column().align(Align::Center);
        if let Some(id) = self.id {
            root = root.id(id);
        }
        if reveal_h > 0 {
            // Spinner zone: height follows the pull; the spinner itself keeps
            // its size and clips into view (the classic reveal).
            let spinner = Spinner::new().size(22).phase(self.phase).build(tokens);
            let zone = clamp_height(
                Panel::column().align(Align::Center).child(spinner).build(),
                reveal_h,
            );
            root = root.child(zone);
        }
        if let Some(content) = self.content {
            root = root.child(content);
        }
        root.build()
    }
}

/// Clamp a node to a fixed height (the reveal window).
fn clamp_height(node: LayoutNode, h: i32) -> LayoutNode {
    match node {
        LayoutNode::Stack(mut s, v, c) => {
            s.min_height = Some(FxPx::new(h));
            s.max_height = Some(FxPx::new(h));
            s.overflow = nexus_layout_types::Overflow::Hidden;
            LayoutNode::Stack(s, v, c)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    fn child_count(node: &LayoutNode) -> usize {
        match node {
            LayoutNode::Stack(_, _, c) => c.len(),
            _ => 0,
        }
    }

    #[test]
    fn resting_shows_content_only() {
        let n = Refresher::new()
            .content(Panel::column().build())
            .build(&BaseTokens);
        assert_eq!(child_count(&n), 1);
    }

    #[test]
    fn pull_reveals_proportionally_and_refresh_holds_full() {
        let reveal = |r: Refresher| r.reveal_height();
        assert_eq!(reveal(Refresher::new().progress(0)), 0);
        assert_eq!(reveal(Refresher::new().progress(50)), REVEAL_H / 2);
        assert_eq!(reveal(Refresher::new().progress(100)), REVEAL_H);
        assert_eq!(reveal(Refresher::new().refreshing(true)), REVEAL_H);
    }

    #[test]
    fn refreshing_prepends_the_spinner_zone() {
        let n = Refresher::new()
            .refreshing(true)
            .content(Panel::column().build())
            .build(&BaseTokens);
        assert_eq!(child_count(&n), 2, "reveal zone + content");
    }
}
