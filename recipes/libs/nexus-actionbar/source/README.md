# nexus-actionbar

A minimal, themable **top action bar** with slide-in **Panels** for Open Nexus on Redox OS.

* Always-on **bar** (clock + two buttons)
* Left **Notifications** panel
* Right **Control Center** panel
* **Instant** or animated open/close (via `libnexus` timelines)
* Fully **theme-driven** (colors, acrylic, icons) through `libnexus`

This crate is a **library** (no event loop). The launcher owns the windows and feeds events into the bar.

---

## How it fits together

* **nexus-actionbar**: draws the bar and panels, handles input, and emits high-level messages.
* **nexus-launcher**: hosts the bar, owns the event loop & windows, reacts to bar messages (e.g., toggles, dismiss).
* **nexus-settingsd**: persists user choices (Desktop/Mobile, Light/Dark). The launcher writes settings when asked.
* **libnexus**: provides theming (paints, icons, acrylic), layout helpers (dp→px, insets), and timeline animations.

```
[user input] → launcher events → nexus-actionbar
                                 │
                                 ├─ emits ActionBarMsg → launcher
                                 │                         ├─ updates nexus-settingsd
                                 │                         └─ applies theme/mode via libnexus
                                 │
                                 └─ uses libnexus THEME for paints/icons, layout, easing
```

---

## Features

* **Bar** with two buttons:

  * Notifications (opens left panel)
  * Control Center (opens right panel)
* **Control Center** exposes two toggles:

  * **UI Mode**: Desktop ⇄ Mobile
  * **Theme Mode**: Light ⇄ Dark
* **Panels** respect **top insets** and a configurable **bottom gap**
  (e.g., don’t overlap a bottom menubar on desktop; do overlap on mobile).
* **Theming** via `libnexus::themes::THEME`:

  * paints like `actionbar_bg`, `panel.notifications.bg`, `panel.controlcenter.bg`
  * optional **acrylic** when configured in `colors.toml`
  * icons resolved from the theme (e.g., `notifications.button`, `controlcenter.button`, `controlcenter.light`, `controlcenter.dark`, `controlcenter.mode`)
* **Animations**: slide timelines (or instant if `reduced_motion`)

---

## Install

Add as a path dependency (paths follow the Redox cookbook layout):

```toml
# Cargo.toml
[dependencies]
nexus-actionbar = { path = "../../libs/nexus-actionbar/source" }
libnexus        = { path = "../../libs/libnexus/source", default-features = false, features = ["svg"] }
```

---

## Quick integration (launcher side)

```rust
use orbclient::{Renderer, Event};
use nexus_actionbar::{ActionBar, Config, ActionBarMsg};
use libnexus::{AnimationManager};

fn run(mut win: impl Renderer, screen_w: u32, screen_h: u32) {
    // 1) Configure
    let cfg = Config {
        // sizes in dp; converted to px internally
        height_dp: 35,
        reduced_motion: false,
        anim_duration_ms: 250,
        easing: libnexus::Easing::CubicOut,
        icon_notifications: "notifications.button".into(),
        icon_control_center:"controlcenter.button".into(),
        // let paints resolve from THEME
        bar_bg: None, button_hover_veil: None, panel_bg: None,
        notifications_width_dp: 360,
        control_center_width_dp: 420,
        // optional bottom gaps if your desktop has a bottom bar:
        bottom_gap_desktop_dp: 54,
        bottom_gap_mobile_dp: 0,
        // initial modes (read from nexus-settingsd in a real app)
        initial_ui_mode: nexus_actionbar::UIMode::Desktop,
        initial_theme_mode: nexus_actionbar::ThemeMode::Light,
    };

    // 2) Create bar + (optional) animation manager
    let mut bar = ActionBar::new(cfg);
    let mut anim = AnimationManager::new();
    bar.set_animation_manager(&mut anim);

    // 3) Reserve top inset for your window manager using the bar’s requirement
    let dpi = 1.0; // or your DPI scale
    let _insets = bar.required_insets(screen_w, screen_h, dpi);

    // 4) Event loop sketch
    loop {
        // ... poll platform events
        let ev: Event = /* read next event */ unimplemented!();

        // a) let the bar handle input
        if let Some(msg) = bar.handle_event(&ev) {
            match msg {
                ActionBarMsg::DismissPanels => {
                    // host may unfocus or just ignore; bar already closes panels
                }
                ActionBarMsg::RequestInsetUpdate(insets) => {
                    // update WM insets if you support dynamic heights
                    let _ = insets;
                }
                ActionBarMsg::RequestSetMode(mode) => {
                    // persist via nexus-settingsd, then re-layout UI
                    // (launcher responsibility)
                }
                ActionBarMsg::RequestSetTheme(theme) => {
                    // persist via nexus-settingsd, reload THEME as needed,
                    // and force a redraw
                }
            }
        }

        // b) Advance animations (if enabled)
        anim.tick(16); // ~60fps; or feed real dt

        // c) Draw
        bar.render_bar(&mut win, 0, screen_w);                 // top bar
        bar.render_panels(&mut win, screen_w, screen_h);       // overlay panels
    }
}
```

> The bar does not own windows; it draws into the renderer(s) you provide.
> If you separate “bar” and “panels” into different transparent windows, call the two render functions with the appropriate render targets.

---

## Public API (high level)

```rust
pub struct ActionBar { /* wraps ActionBarState */ }

impl ActionBar {
    pub fn new(cfg: Config) -> Self;

    /// Insets your WM should reserve (top bar height in px).
    pub fn required_insets(&self, screen_w: u32, screen_h: u32, dpi: f32) -> libnexus::ui::Insets;

    /// Advance animations (if you use AnimationManager).
    pub fn update(&mut self, dt_ms: u32);

    /// Returns true if any timeline is currently running.
    pub fn is_animating(&self) -> bool;

    /// Feed a single OrbClient event; returns an optional message for the host.
    pub fn handle_event(&mut self, ev: &orbclient::Event) -> Option<ActionBarMsg>;

    /// Draw the bar (background + buttons).
    pub fn render_bar<R: orbclient::Renderer>(&mut self, win: &mut R, y: i32, w: u32);

    /// Draw overlay panels (Notifications / Control Center).
    pub fn render_panels<R: orbclient::Renderer>(&mut self, win: &mut R, screen_w: u32, screen_h: u32);

    /// Close panels programmatically (e.g., when opening the Start menu).
    pub fn dismiss_panels(&mut self);

    /// Hook up `libnexus::AnimationManager` for 60fps timeline driving.
    pub fn set_animation_manager(&mut self, manager: &mut libnexus::AnimationManager);
}

// Messages to the host
#[derive(Clone, Debug)]
pub enum ActionBarMsg {
    DismissPanels,
    RequestInsetUpdate(libnexus::ui::Insets),
    RequestSetMode(UIMode),
    RequestSetTheme(ThemeMode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UIMode    { Desktop, Mobile }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode { Light, Dark }
```

---

## Config reference

All dimensions are **dp** (density-independent pixels). The bar converts to px via `libnexus::ui::layout::conversion::dp_to_px`.

```rust
pub struct Config {
    // Behavior
    pub height_dp: u32,               // default 35
    pub reduced_motion: bool,         // true → instant toggles (no animation)
    pub anim_duration_ms: u32,        // 250 by default
    pub easing: libnexus::Easing,     // e.g., CubicOut

    // Icons (theme IDs resolved by THEME)
    pub icon_notifications: String,   // "notifications.button"
    pub icon_control_center: String,  // "controlcenter.button"

    // Optional paint overrides (otherwise resolved by THEME)
    pub bar_bg: Option<orbclient::Color>,
    pub button_hover_veil: Option<orbclient::Color>,
    pub panel_bg: Option<orbclient::Color>,

    // Panels (widths in dp, clamped at runtime)
    pub notifications_width_dp: u32,  // default 360
    pub control_center_width_dp: u32, // default 420

    // Optional bottom gaps per UI mode (in dp)
    pub bottom_gap_desktop_dp: u32,   // e.g., 54 if you have a bottom menubar
    pub bottom_gap_mobile_dp: u32,    // often 0 on mobile

    // Initial modes (typically read from nexus-settingsd)
    pub initial_ui_mode: UIMode,          // Desktop/Mobile
    pub initial_theme_mode: ThemeMode,    // Light/Dark
}
```

**Theming keys** (conventional, but you can map differently in your theme):

* `actionbar_bg` (paint) — bar background; may include acrylic
* `panel.notifications.bg` (paint)
* `panel.controlcenter.bg` (paint)
* `button_hover_veil` (paint) — subtle hover rectangle behind icons
* Icons:

  * `notifications.button`
  * `controlcenter.button`
  * `controlcenter.light`, `controlcenter.dark` (toggle visuals)
  * `controlcenter.mode` (desktop/mobile glyph)

If a paint has an **acrylic** section in `colors.toml`, the launcher can render panels through the acrylic helper (blur+tint) using a wallpaper/backdrop. The actionbar’s code paths are prepared for acrylic-aware paints.

---

## Persisting toggles

The bar **does not write settings**. When the user taps a toggle in the **Control Center**, it emits:

* `ActionBarMsg::RequestSetMode(UIMode)`
* `ActionBarMsg::RequestSetTheme(ThemeMode)`

Your launcher should:

1. Update its in-memory state (re-layout if UI mode changes).
2. Persist via `nexus-settingsd` (`set_enum(..)`, `save()`).
3. Re-apply theme (`libnexus::themes::THEME`) and redraw.

---

## Panels & layout

* Panels slide under the top bar and stop above the **bottom gap** (per mode).
* In **Desktop** mode you usually set a positive bottom gap to avoid overlapping a bottom menubar.
* In **Mobile** mode the bottom gap is often `0` (panels can cover the full height under the top bar).

---

## DPI & insets

* Provide your **DPI scale** to `required_insets()` / layout; internally the bar uses `dp_to_px()`.
* Use the returned **top inset** so application windows don’t overlap the bar.

---

## Acrylic notes

If your theme enables acrylic in `colors.toml`, the host should draw panels with the acrylic helper (blur + tint + optional noise) sampling the current desktop backdrop. This keeps the bar library simple and lets the host decide how to obtain the backdrop (e.g., from wallpaper, cached composition, etc.).

---

## Roadmap

* Hook up the animation manager by default (opt-in today)
* Optional keyboard navigation focus for panels
* Rich notification items & actions
* More Control Center toggles (Wi-Fi, BT, Do Not Disturb, etc.)

---

## License

Apache-2.0 © the Open Nexus authors.
