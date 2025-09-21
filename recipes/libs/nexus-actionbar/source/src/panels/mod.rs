//! Panels overlay rendering and composition.

use orbclient::{Renderer};
use crate::ui::state::{ActionBarState, PanelOpen};

pub mod notifications;
pub mod control_center;

/// Render visible panels as overlays. Notifications slide from the left,
/// Control Center slides from the right. Both can never be open at the same time.
pub fn render<R: Renderer>(state: &mut ActionBarState, win: &mut R, screen_w: u32, screen_h: u32) {
    match state.open {
        PanelOpen::Notifications => {
            notifications::render(state, win, screen_w, screen_h);
        }
        PanelOpen::ControlCenter => {
            control_center::render(state, win, screen_w, screen_h);
        }
        PanelOpen::None => {
            // If neither is open, we may still be animating out; draw if progress > 0.
            if state.tl_notifications.value() > 0.0 {
                notifications::render(state, win, screen_w, screen_h);
            } else if state.tl_control.value() > 0.0 {
                control_center::render(state, win, screen_w, screen_h);
            }
        }
    }
}
