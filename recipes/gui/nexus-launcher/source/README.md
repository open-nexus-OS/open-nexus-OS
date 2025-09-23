# Nexus Launcher

Nexus Launcher is the system “shell” process for Open Nexus on Redox OS.
It draws the bottom taskbar, the Start menu(s), and hosts the top **Action Bar** with slide-in panels (Notifications & Control Center). The launcher is a thin coordinator: UI widgets, layout helpers and theming live in **libnexus**, while persistent user preferences (mobile/desktop mode, dark/light theme, etc.) are handled through **nexus-settingsd**.

> TL;DR: the launcher starts background services (wallpaper, etc.), renders the bar, forwards input to the Action Bar, and reacts to state changes by updating the UI and saving settings.

---

## Architecture at a glance

```
+-----------------------------+       +----------------------+
|         nexus-launcher      |<----->|   nexus-settingsd    |
|  (this repo, the "host")    |   RW  |  (lib, persistent    |
|                             |       |   user preferences)  |
+-------------+---------------+       +----------------------+
              | uses
              v
+-----------------------------+       +----------------------+
|          libnexus           |       |   nexus-actionbar    |
|  (themes, paints, layout,   |<----->|  (bar + panels lib)  |
|   animation, SVG icons)     |       |  renders top bar     |
+-----------------------------+       |  and side panels     |
                                      +----------------------+
```

### Responsibilities

* **nexus-launcher**

  * Spawns long-running helpers (e.g. `nexus-background`)
  * Draws the bottom taskbar & Start menu (desktop/mobile variants)
  * Hosts an instance of `nexus_actionbar::ActionBar` and wires its events
  * Applies and persists mode/theme changes via `nexus-settingsd`
  * Keeps windows insets in sync (so apps don’t overlap bars/panels)

* **nexus-actionbar (library)**

  * Renders the 35dp top bar with two toggle buttons
  * Slide-in panels: Notifications (left) and Control Center (right)
  * Emits messages to the host, e.g.
    `DismissPanels`, `RequestSetMode(Desktop|Mobile)`, `RequestSetTheme(Light|Dark)`

* **libnexus**

  * **Theme Manager** (icon resolution, colors/paints, backgrounds, caching)
  * **Acrylic** effects (cheap blur + tint + noise), DPI helpers, layout utils
  * **Animation** timelines & easing (the launcher can run with or without)
  * SVG rendering (when the `svg` feature is enabled)

* **nexus-settingsd (library)**

  * Small, synchronous settings store (key/value), backed by a simple file
  * Used by the launcher to persist “UI mode” and “theme mode” across boots

---

## Feature overview

* Bottom taskbar with centered app icons and clock
* Start menu (desktop: small/large, mobile: full-screen)
* Top **Action Bar** with:

  * **Notifications** panel (left)
  * **Control Center** (right) with two primary toggles:

    * **Desktop/Mobile** mode
    * **Light/Dark** theme
* Theming via `/ui/nexus.toml` + `/ui/themes/<light|dark>/colors.toml`
* **Acrylic glass** surfaces using theme paints (blur-approx, tint & noise)
* DPI-aware metrics (dp → px conversion)
* Safe insets so side panels don’t overlap the bottom bar in desktop mode

---

## How components talk to each other

### ActionBar → Launcher (messages)

The bar emits typed messages; the launcher reacts:

* `DismissPanels` – close any open panels
* `RequestInsetUpdate(Insets)` – adjust reserved screen areas
* `RequestSetMode(UIMode)` – user toggled Desktop/Mobile in Control Center
* `RequestSetTheme(ThemeMode)` – user toggled Light/Dark in Control Center

In the launcher, `handle_bar_msg(...)` applies the change (update UI, persist via `nexus-settingsd`), then requests a redraw.

### Theming & paints (libnexus)

* The launcher and actionbar **never hard-code colors**. They ask:

  ```rust
  let p = THEME.paint("actionbar_bg", Paint { color: fallback, acrylic: None });
  ```
* The **paint** contains a base `Color` and optional `Acrylic` config.
  When acrylic is present, panels render a blurred+tinted patch, otherwise a flat fill.

### Persistence (nexus-settingsd)

* Launcher reads initial `ui_mode` and `theme_mode` on startup.
* When the Control Center toggles a switch, the launcher writes the new value.
* On next boot the previous state is restored.

---

## Directory layout (launcher)

```
source/
├─ src/
│  ├─ main.rs              # entry point (logging, screen size, args)
│  ├─ lib.rs               # crate exports (ActionBar wrapper etc.)
│  ├─ ui/
│  │  ├─ bar_handler.rs    # bottom taskbar loop & Start menu hook
│  │  ├─ menu_handler.rs   # desktop/mobile start menus
│  │  └─ bar_msg.rs        # applies ActionBar messages (settings, theme)
│  ├─ modes/
│  │  ├─ desktop.rs        # desktop start menu
│  │  └─ mobile.rs         # mobile start menu
│  ├─ services/
│  │  ├─ app_catalog.rs    # discover apps & icons
│  │  ├─ process_manager.rs# spawn/wait helpers
│  │  └─ background_service.rs # acrylic backdrop
│  ├─ config/
│  │  ├─ settings.rs       # simple runtime switches (bar height, scales)
│  │  └─ colors.rs         # paint getters resolving via THEME
│  └─ utils/
│     └─ dpi_helper.rs     # dp/px helpers for fonts
└─ Cargo.toml
```

---

## Building & running (Redox OS)

The launcher is built by Redox’ **cookbook** as part of the desktop recipe.

```bash
# from redox/ root
make desktop
# or specifically rebuild the launcher recipe:
cook -p recipes/gui/nexus-launcher
```

If you are iterating locally inside the recipe, you can run the cookbook redoxer:

```bash
/mnt/redox/cookbook/target/release/cookbook_redoxer install \
  --path /mnt/redox/cookbook/recipes/gui/nexus-launcher/source/. \
  --root /mnt/redox/cookbook/recipes/gui/nexus-launcher/target/.../stage.tmp/usr \
  --locked --no-track -j 8 --bin nexus-launcher
```

> **Note:** `libnexus` is used with the `svg` feature. Ensure your `Cargo.toml` contains:
>
> ```toml
> libnexus = { path = "../../libs/libnexus/source", default-features = false, features = ["svg"] }
> ```

---

## Theming

### Files

* `/ui/nexus.toml`
  Declares the active theme (`light` / `dark`), and resolves icon/background IDs.
* `/ui/themes/light/colors.toml`, `/ui/themes/dark/colors.toml`
  Define named colors or **paints** (color + optional acrylic).

### Example `colors.toml`

```toml
[defaults.acrylic]
enabled = true
downscale = 4
tint = "#33222222"      # ARGB-like hex supported
noise_alpha = 16

[colors]
actionbar_bg = "#FFFFFF59"
panel_bg = { rgba = [0, 0, 0, 89], acrylic = { enabled = true } }

# optional more keys used by launcher/actionbar
button_hover_veil = [0, 0, 0, 26]
control_center_group_bg = [255, 255, 255, 13]
notification_pill_bg = [0, 0, 0, 51]
```

### Acrylic “glass”

If a paint contains `acrylic`, the launcher/actionbar will:

1. Grab or synthesize a backdrop (theme background scaled to the screen)
2. Downscale+upscale (cheap blur), apply a tint, sprinkle noise
3. Optionally overlay the paint’s color on top (for readability/opacity)

This gives the look of a semi-transparent rectangle where you can “see through” without needing live screen readback.

---

## Using the launcher

* **Start menu:** click the start icon in the bottom bar.
  Desktop shows small/large menus (rounded corners), Mobile shows a full-screen list.
* **Notifications panel:** top-left bell button.
* **Control Center:** top-right button.
  Contains two toggles:

  * **Desktop/Mobile mode** (icon only)
  * **Light/Dark theme** (icon switches sun/moon and muted/active tone)

Panels in **Desktop** respect a bottom gap so they do not cover the bottom bar; in **Mobile** they extend to the bottom.

---

## Extending the launcher

* Add a new toggle to Control Center

  1. Draw a button in `nexus-actionbar/panels/control_center.rs`
  2. Emit a new `ActionBarMsg::RequestSet…` variant
  3. Handle it in the launcher’s `ui/bar_msg.rs` and persist via `nexus-settingsd`
* Add a new paint or color

  1. Define it in `colors.toml`
  2. Resolve via `THEME.paint("your_key", fallback)`
  3. If acrylic is present, you’ll get the blurred overlay automatically
* Add icons

  * Place SVG/PNG in your theme’s icon tree, map logical IDs in `nexus.toml`, then use `THEME.load_icon_sized("logical.id", ...)`.

---

## Troubleshooting

* **Blank icons**
  Ensure `libnexus` is built with `features = ["svg"]` or provide PNG fallbacks.
* **Panels overlap bottom bar on desktop**
  Check `bottom_gap_desktop_dp` in `nexus-actionbar::config::Config`. The launcher’s bottom bar height is 54px by default; we set the gap to match.
* **No acrylic effect**
  Verify your `colors.toml` paint has `acrylic.enabled = true`. If no theme background is found, a flat fill is used.
* **Mode/theme not remembered**
  Confirm the launcher has access to `nexus-settingsd` (the library) and that writes succeed. The launcher calls settings APIs when handling `RequestSetMode/RequestSetTheme`.

---

## License

Apache-2.0 © the Open Nexus authors. See `LICENSE` for details.
