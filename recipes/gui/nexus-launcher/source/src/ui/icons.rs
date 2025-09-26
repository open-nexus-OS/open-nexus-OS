use orbimage::Image;
use libnexus::themes::THEME;

/// A small icon collection used by the start menu (desktop + mobile).
///
/// Naming convention:
/// - *_sm  → icons for the small desktop menu (bright background → black icons)
/// - *_lg  → icons for the large desktop menu and mobile (dark overlay → white icons)
/// - `user` uses a neutral avatar from the icon theme tree.
pub struct CommonIcons {
    pub power_sm: Image,
    pub power_lg: Image,
    pub settings_sm: Image,
    pub settings_lg: Image,
    pub search_sm: Image,
    pub search_lg: Image,
    pub resize_sm: Image, // shows "bigger" in small menu (action = expand)
    pub resize_lg: Image, // shows "smaller" in large menu (action = shrink)
    pub user: Image,
}

impl CommonIcons {
    /// Load all icons from nexus-assets via libnexus theme system.
    pub fn load(_ui_path: &str) -> Self {
        // Load icons from nexus-assets using the theme system
        let load_icon = |name: &str, size: u32| {
            THEME.load_icon_sized(name, libnexus::themes::IconVariant::Auto, Some((size, size))).unwrap_or(Image::default())
        };

        Self {
            // Contrast logic:
            //  - small desktop menu → bright background → black icons
            //  - large desktop & mobile → dark overlay → white icons
            power_sm:    load_icon("power.shutdown", 24),
            power_lg:    load_icon("power.shutdown", 24),

            settings_sm: load_icon("settings", 24),
            settings_lg: load_icon("settings", 24),

            search_sm:   load_icon("menu.search", 24),
            search_lg:   load_icon("menu.search", 24),

            // Toggle between small/large menu:
            //  - in small menu we show "bigger"
            //  - in large menu we show "smaller"
            resize_sm:   load_icon("menu.bigger", 24),
            resize_lg:   load_icon("menu.smaller", 24),

            // Avatar is typically provided in the icon tree
            user:        load_icon("avatar", 32),
        }
    }
}


