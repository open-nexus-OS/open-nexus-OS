// themes/mod.rs — central hub that wires submodules together.
pub mod colors;     // color types, parsing, and theme color files
pub mod svg_icons;  // icon resolution, SVG render, raster scaling
pub mod manager;    // ThemeManager: loads nexus.toml, caches, exposes API
pub mod effects;    // optional visual effects (e.g., acrylic approximation)

// Public re-exports to provide a single import surface:
pub use colors::{Paint, Acrylic};
pub use manager::{ThemeManager, THEME, ThemeId};
pub use svg_icons::IconVariant;
