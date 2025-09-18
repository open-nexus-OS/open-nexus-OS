pub mod themes;

// This lets the rest of your code keep using `use libnexus::{THEME, IconVariant};`
pub use themes::{
    THEME,
    ThemeManager,
    ThemeId,
    IconVariant,
    Paint,
    Acrylic,
};
