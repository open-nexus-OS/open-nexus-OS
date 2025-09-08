use orbimage::Image;

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
    pub resize_sm: Image, // shows “bigger” in small menu (action = expand)
    pub resize_lg: Image, // shows “smaller” in large menu (action = shrink)
    pub user: Image,
}

impl CommonIcons {
    /// Load all icons relative to `ui_path`.
    ///
    /// Important:
    /// - `ui_path` should be the **root** path for UI assets
    ///   (e.g. `"/ui"` on Redox, `"ui"` on other targets).
    /// - Pass only the *relative* filename inside that root here.
    ///
    /// Example expected paths (joined as `format!("{}/{}", ui_path, name)`):
    /// - `/ui/turn-off-black.png`
    /// - `/ui/icons/system/avatar.png`
    pub fn load(ui_path: &str) -> Self {
        let p = |name: &str| {
            let full = format!("{}/{}", ui_path, name);
            // In this codebase, Image::from_path returns a Result; fall back to an empty image.
            Image::from_path(full).unwrap_or(Image::default())
        };

        Self {
            // Contrast logic:
            //  - small desktop menu → bright background → black icons
            //  - large desktop & mobile → dark overlay → white icons
            power_sm:    p("turn-off-black.png"),
            power_lg:    p("turn-off-white.png"),

            settings_sm: p("settings-black.png"),
            settings_lg: p("settings-white.png"),

            search_sm:   p("search-black.png"),
            search_lg:   p("search-white.png"),

            // Toggle between small/large menu:
            //  - in small menu we show “bigger”
            //  - in large menu we show “smaller”
            resize_sm:   p("bigger-black.png"),
            resize_lg:   p("smaller-white.png"),

            // Avatar is typically provided in the icon tree
            user:        p("icons/system/avatar.png"),
        }
    }
}
