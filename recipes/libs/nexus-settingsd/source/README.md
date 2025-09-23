# nexus-settingsd

A tiny, embeddable settings **library** for Open Nexus on Redox OS.

Despite the name, **nexus-settingsd is not a daemon**. It’s a synchronous, file-backed key/value store with a small Rust API. It’s used by `nexus-launcher` (and can be used by other components) to persist user choices such as **Desktop/Mobile** UI mode and **Light/Dark** theme across reboots.

* No background process
* No IPC
* Just `open → get/set → save`

---

## Why a separate crate?

* **Reuse:** the launcher, setup tools, and future apps can all read/write the same settings.
* **Stability:** a single, documented file format and keys.
* **Simplicity:** keep persistence out of the UI/event loops.

---

## Quick start

Add the dependency (path example matches the Redox cookbook layout):

```toml
# Cargo.toml
[dependencies]
nexus-settingsd = { path = "../../libs/nexus-settingsd/source" }
```

Read the current UI mode & theme, then toggle and persist:

```rust
use nexus_settingsd::{Settings, Key};

fn example() -> Result<(), Box<dyn std::error::Error>> {
    // Open the default settings file (see “Storage location” below).
    let mut s = Settings::open_or_default()?;

    // Read with defaults
    let ui_mode: String   = s.get_string(Key::UiMode, "desktop");
    let theme_mode: String= s.get_string(Key::ThemeMode, "light");

    // Toggle something
    let new_theme = if theme_mode == "light" { "dark" } else { "light" };
    s.set_string(Key::ThemeMode, new_theme)?;

    // Flush to disk
    s.save()?;
    Ok(())
}
```

Using typed enums is also supported if you prefer stronger types:

```rust
use nexus_settingsd::{Settings, UiMode, ThemeMode, Key};

let mut s = Settings::open_or_default()?;
let mode  = s.get_enum(Key::UiMode, UiMode::Desktop);
let theme = s.get_enum(Key::ThemeMode, ThemeMode::Light);

s.set_enum(Key::UiMode, UiMode::Mobile)?;
s.save()?;
```

---

## Integration points

### With `nexus-launcher`

* At startup, the launcher calls `Settings::open_or_default()` and reads:

  * `Key::UiMode` → `desktop|mobile`
  * `Key::ThemeMode` → `light|dark`
* When the **Action Bar**’s Control Center asks for a change (via `ActionBarMsg::RequestSetMode/RequestSetTheme`), the launcher:

  1. Updates its in-memory state
  2. Calls `set_enum(...)` + `save()` on `nexus-settingsd`
  3. Re-renders UI (and updates the theme via `libnexus::themes::THEME`)

### With `nexus-actionbar`

* The bar **does not** persist anything itself.
  It only emits requests; the launcher decides and persists via `nexus-settingsd`.

### With `libnexus`

* `libnexus` consumes the **current** theme selection (light/dark) the launcher applies; it does not read the settings file directly.

---

## API surface

```rust
pub struct Settings { /* opaque */ }

impl Settings {
    /// Open an existing settings file or create with defaults.
    pub fn open_or_default() -> std::io::Result<Self>;

    /// Open from an explicit path.
    pub fn with_path<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Self>;

    /// Generic getters with default fallbacks.
    pub fn get_string(&self, key: Key, default: &str) -> String;
    pub fn get_bool(&self,   key: Key, default: bool) -> bool;
    pub fn get_enum<E>(&self, key: Key, default: E) -> E
        where E: std::str::FromStr + ToString + Copy;

    /// Setters (buffered; call `save()` to persist).
    pub fn set_string<S: Into<String>>(&mut self, key: Key, value: S) -> std::io::Result<()>;
    pub fn set_bool(&mut self, key: Key, value: bool) -> std::io::Result<()>;
    pub fn set_enum<E: ToString>(&mut self, key: Key, value: E) -> std::io::Result<()>;

    /// Persist to storage.
    pub fn save(&self) -> std::io::Result<()>;
}

/// Well-known keys used across Nexus.
#[derive(Copy, Clone, Debug)]
pub enum Key {
    UiMode,                 // "desktop" | "mobile"
    ThemeMode,              // "light" | "dark"
    BottomGapDesktopDp,     // u32 (optional)
    BottomGapMobileDp,      // u32 (optional)
    // add more keys over time (names remain stable)
}

/// Optional typed enums (helpers).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UiMode   { Desktop, Mobile }
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ThemeMode{ Light, Dark }
```

> The library converts enums to strings in the file for human readability.

---

## Storage location

By default the crate chooses the first writable location:

1. `$NEXUS_SETTINGS_PATH` (if set), or
2. `~/.config/nexus/settings.toml`, or
3. `/etc/nexus/settings.toml` (system-wide fallback; may be read-only)

You can always force a specific path via `Settings::with_path(path)`.

---

## File format

Human-readable **TOML**:

```toml
# ~/.config/nexus/settings.toml
[ui]
mode  = "desktop"          # "desktop" | "mobile"
theme = "dark"             # "light" | "dark"

[gaps]                      # optional; actionbar/panels bottom spacing
desktop_dp = 54
mobile_dp  = 0
```

The library tolerates missing keys and will supply defaults you pass to `get_*()`.

---

## Concurrency model

* Intended for **single-process** writes (e.g., the launcher).
* Readers in other processes are fine (they can re-open on demand).
* If you need multi-writer coordination later, add a coarse file lock around `save()` in your caller; the crate keeps the surface small.

---

## Versioning & compatibility

* Keys are **stable** once shipped. New keys are additive.
* TOML structure is flat and forgiving; unknown keys are ignored by callers that don’t use them.

---

## Testing locally

```rust
#[test]
fn round_trip() {
    use nexus_settingsd::{Settings, Key, UiMode};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");

    let mut s = Settings::with_path(&path).unwrap();
    assert_eq!(s.get_enum(Key::UiMode, UiMode::Desktop), UiMode::Desktop);

    s.set_enum(Key::UiMode, UiMode::Mobile).unwrap();
    s.save().unwrap();

    let s2 = Settings::with_path(&path).unwrap();
    assert_eq!(s2.get_string(Key::UiMode, "desktop"), "mobile");
}
```

---

## Roadmap

* Optional notify/watch API (callbacks on external file changes)
* Namespaced keys for apps (`app.<id>.<key>`)
* Pluggable backends (TOML/JSON), kept behind the same API

---

## License

Apache-2.0 © the Open Nexus authors.
