// Light/Dark Themes

use orbclient::Color;

pub const BAR_COLOR: Color = Color::rgba(0xFF, 0xFF, 0xFF, 191);
pub const BAR_HIGHLIGHT_COLOR: Color = Color::rgba(0xFF, 0xFF, 0xFF, 224);
pub const TEXT_COLOR: Color = Color::rgb(0x00, 0x00, 0x00);
pub const TEXT_HIGHLIGHT_COLOR: Color = Color::rgb(0x14, 0x14, 0x14);

pub const BAR_HEIGHT: u32 = 57;      // bar height
pub const ICON_SCALE: f32 = 0.5;     // ½ of the bar height for icons
pub const ICON_PADDING: u32 = 16;    // padding around icons
