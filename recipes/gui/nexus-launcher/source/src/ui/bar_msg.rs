//! Routing of ActionBar messages to system settings + theme switch.

use libnexus::themes::{THEME, ThemeId};
use nexus_actionbar::ActionBarMsg;
use nexus_actionbar::config::{UIMode as BarUIMode, ThemeMode as BarThemeMode};
use nexus_settingsd::{self as settingsd, UIMode as SysUIMode, ThemeMode as SysThemeMode};
use crate::config::settings as app_settings;

/// Apply persisted settings (UI mode + theme) on startup.
pub fn apply_initial_settings() {
    let (ui_mode, theme_mode) = settingsd::get_all();

    // UI mode → app-local atomic (so menus/layout reagieren)
    let app_mode = match ui_mode {
        SysUIMode::Desktop => app_settings::Mode::Desktop,
        SysUIMode::Mobile  => app_settings::Mode::Mobile,
    };
    app_settings::set_mode(app_mode);

    // Theme live umschalten
    match theme_mode {
        SysThemeMode::Light => THEME.switch_theme(ThemeId::Light),
        SysThemeMode::Dark  => THEME.switch_theme(ThemeId::Dark),
    }
    log::debug!("Launcher init: ui={:?}, theme={:?}", ui_mode, theme_mode);
}

/// Handle a single message coming back from the ActionBar.
pub fn handle_bar_msg(msg: ActionBarMsg) {
    match msg {
        ActionBarMsg::RequestSetMode(mode) => {
            let sys = match mode {
                BarUIMode::Desktop => SysUIMode::Desktop,
                BarUIMode::Mobile  => SysUIMode::Mobile,
            };
            settingsd::set_ui_mode(sys);

            let app = match sys {
                SysUIMode::Desktop => app_settings::Mode::Desktop,
                SysUIMode::Mobile  => app_settings::Mode::Mobile,
            };
            app_settings::set_mode(app);
            log::debug!("Launcher: UI mode -> {:?}", sys);
        }

        ActionBarMsg::RequestSetTheme(theme) => {
            let sys = match theme {
                BarThemeMode::Light => SysThemeMode::Light,
                BarThemeMode::Dark  => SysThemeMode::Dark,
            };
            settingsd::set_theme_mode(sys);

            match sys {
                SysThemeMode::Light => THEME.switch_theme(ThemeId::Light),
                SysThemeMode::Dark  => THEME.switch_theme(ThemeId::Dark),
            }
            log::debug!("Launcher: theme -> {:?}", sys);
        }

        ActionBarMsg::RequestInsetUpdate(insets) => {
            crate::config::settings::set_top_inset(insets.top);
        }

        ActionBarMsg::DismissPanels => {
            // no-op for now; ActionBar managed
        }
    }
}
