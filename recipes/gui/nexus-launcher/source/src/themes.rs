// Light/Dark Themes

use orbclient::Color;

pub const BAR_COLOR: Color = Color::rgba(0xFF, 0xFF, 0xFF, 191);
pub const BAR_HIGHLIGHT_COLOR: Color = Color::rgba(0xFF, 0xFF, 0xFF, 200);
pub const BAR_ACTIVITY_MARKER: Color = Color::rgba(0xFF, 0xFF, 0xFF, 224);
pub const TEXT_COLOR: Color = Color::rgba(0x00, 0x00, 0x00, 255);
pub const TEXT_HIGHLIGHT_COLOR: Color = Color::rgba(0x14, 0x14, 0x14, 255);

pub const BAR_HEIGHT: u32 = 53;      // bar height
pub const ICON_SCALE: f32 = 0.65;     // 65% of the bar height for icons
pub const ICON_SMALL_SCALE: f32 = 0.75; // 75% of the bar height for small icons
