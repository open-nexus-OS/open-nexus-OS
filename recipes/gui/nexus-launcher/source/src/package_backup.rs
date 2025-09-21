// src/package.rs
// App definitions and icon loading via libnexus ThemeManager only.
// No local SVG/raster fallback here — if the theme can't resolve an icon,
// we just return a tiny transparent image (caller can decide how to handle it).

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use orbimage::Image;
use libnexus::themes::{THEME, IconVariant};

/// Icon source indicates whether an icon is a logical theme name or a raw path.
/// We normalize paths to a best-effort logical id when possible.
#[derive(Clone, Debug)]
pub enum IconSource {
    None,
    Name(String),
    Path(PathBuf),
}

impl IconSource {
    pub fn get_name(&self) -> Option<&str> {
        match self {
            IconSource::Name(name) => Some(name),
            _ => None,
        }
    }
}

/// Icon holds a per-size cache keyed by (target_px, dpi*100) for crisp renders.
/// No legacy one-shot bitmap cache remains.
#[derive(Clone)]
pub struct Icon {
    pub source: IconSource,
    small: bool,
    // (target_size_px, dpi_scaled*100) -> Image
    resolution_cache: BTreeMap<(u32, u32), Image>,
}

impl Icon {
    pub fn empty(small: bool) -> Self {
        Self {
            source: IconSource::None,
            small,
            resolution_cache: BTreeMap::new(),
        }
    }

    /// Clears all cached variants for this icon.
    pub fn clear_all_caches(&mut self) {
        self.resolution_cache.clear();
    }

    /// Resolve/derive a logical icon id from the source.
    /// - Name("foo/bar") -> "foo/bar"
    /// - Path("/ui/icons/places/start-here.svg") -> "places/start-here"
    /// - None -> ""
    fn logical_id(&self) -> Option<String> {
        match &self.source {
            IconSource::Name(name) => Some(name.clone()),
            IconSource::Path(p) => {
                // Strip known prefixes and extension to get something the theme can try.
                // e.g. /ui/themes/light/icons/places/start-here.svg -> places/start-here
                let s = p.to_string_lossy();
                let s = s
                    .replace("/ui/themes/light/icons/", "")
                    .replace("/ui/themes/dark/icons/", "")
                    .replace("/ui/icons/", "");
                let s = s.trim_start_matches('/');
                let stem = Path::new(s).with_extension("");
                let stem = stem.to_string_lossy().to_string();
                if stem.is_empty() { None } else { Some(stem) }
            }
            IconSource::None => None,
        }
    }

    /// Return a crisp image for the requested size and DPI, using libnexus THEME only.
    pub fn image_sized(&mut self, target_size: u32, dpi_scale: f32) -> &Image {
        let key = (target_size, (dpi_scale * 100.0) as u32);

        // Compute immutable state **before** taking a mutable entry-borrow.
        let logical = self.logical_id();
        let dpi_scaled = (target_size as f32 * dpi_scale).round().max(1.0) as u32;

        // Use Entry API to avoid immutable+mutable borrow overlap
        match self.resolution_cache.entry(key) {
            Entry::Occupied(e) => {
                // &mut Image coerces to &Image
                let r: &mut Image = e.into_mut();
                r
            }
            Entry::Vacant(v) => {
                // cache miss: render first, then insert and return reference
                // use precomputed values; don't touch `self` again while entry is held
                let rendered = match logical.as_deref() {
                    Some(id) => THEME
                        .load_icon_sized(id, IconVariant::Auto, Some((dpi_scaled, dpi_scaled)))
                        .unwrap_or_else(|| {
                            // Fallback auf apps.unknown
                            THEME.load_icon_sized("apps/unknown", IconVariant::Auto, Some((dpi_scaled, dpi_scaled)))
                                .unwrap_or_else(|| transparent_1x1())
                        }),
                   None => transparent_1x1(),
                };
                let r: &mut Image = v.insert(rendered);
                r
            }
        }
    }
}

/// Tiny 1×1 transparent placeholder (no local theme fallback/IO).
fn transparent_1x1() -> Image {
    use orbclient::Color;
    Image::from_data(1, 1, vec![Color::rgba(0, 0, 0, 0)].into()).unwrap()
}

/// A package displayed by the launcher.
#[derive(Clone)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub categories: BTreeSet<String>,
    pub exec: String,
    pub icon: Icon,
    pub icon_small: Icon,
    pub accepts: Vec<String>,
    pub authors: Vec<String>,
    pub descriptions: Vec<String>,
}

impl Package {
    pub fn new() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            categories: BTreeSet::new(),
            exec: String::new(),
            icon: Icon::empty(false),
            icon_small: Icon::empty(true),
            accepts: Vec::new(),
            authors: Vec::new(),
            descriptions: Vec::new(),
        }
    }

    /// Clear both icon caches — used on mode/size toggles.
    pub fn clear_icon_caches(&mut self) {
        self.icon.clear_all_caches();
        self.icon_small.clear_all_caches();
    }

    /// Returns a themed icon sized appropriately; `large = true` prefers the big icon cache.
    pub fn get_icon_sized(&mut self, target_size: u32, dpi_scale: f32, large: bool) -> &Image {
        if large {
            // Big menu: slightly bias to larger bucket (64–96px typical)
            let px = target_size.clamp(48, 128);
            self.icon.image_sized(px, dpi_scale)
        } else {
            // Small UI (bar/chooser): 24–48px typical
            let px = target_size.clamp(16, 64);
            self.icon_small.image_sized(px, dpi_scale)
        }
    }

    /// Create package from Redox app info file.
    pub fn from_path(path: &str) -> Self {
        let mut package = Package::new();

        for part in path.rsplit('/') {
            if !part.is_empty() {
                package.id = part.to_string();
                break;
            }
        }

        let mut info = String::new();
        if let Ok(mut file) = File::open(path) {
            let _ = file.read_to_string(&mut info);
        }

        for line in info.lines() {
            if let Some(val) = line.strip_prefix("name=") {
                package.name = val.to_string();
            } else if let Some(category) = line.strip_prefix("category=") {
                if !category.is_empty() {
                    package.categories.insert(category.into());
                }
            } else if let Some(bin) = line.strip_prefix("binary=") {
                        // Adds %f for launching files
                if let Ok(binary) = shlex::try_quote(bin) {
                        package.exec = format!("{binary} %f");
                }
            } else if let Some(icon) = line.strip_prefix("icon=") {
                // Normalize path to a logical id (theme name), not a raw PNG/SVG path.
                let stem = Path::new(icon).file_stem().and_then(|s| s.to_str()).unwrap_or(icon);
                package.icon.source = IconSource::Name(stem.into());
                package.icon_small.source = IconSource::Name(stem.into());
            } else if let Some(val) = line.strip_prefix("accept=") {
                package.accepts.push(val.to_string());
            } else if let Some(val) = line.strip_prefix("author=") {
                package.authors.push(val.to_string());
            } else if let Some(val) = line.strip_prefix("description=") {
                package.descriptions.push(val.to_string());
            }
        }

        package
    }

    /// Create package from Freedesktop .desktop entry.
    pub fn from_desktop_entry(id: String, path: &Path) -> Option<Self> {
        let entry = freedesktop_entry_parser::parse_entry(path).ok()?;
        let mut package = Package::new();
        package.id = id;
        let section = entry.section("Desktop Entry");

        if let Some(name) = section.attr("Name") {
            package.name = name.into();
        }

        if let Some(categories) = section.attr("Categories") {
            // main category registry → normalized few
            let main_categories = [
                "AudioVideo", "Audio", "Video", "Development", "Education", "Game", "Graphics",
                "Network", "Office", "Science", "Settings", "System", "Utility",
            ];
            for category in categories.split_terminator(';') {
                if main_categories.contains(&category) {
                    let mapped = match category {
                            "AudioVideo" | "Audio" | "Video" => "Multimedia",
                            "Game" => "Games",
                            _ => category,
                    };
                    package.categories.insert(mapped.into());
                }
            }
        }

        if let Some(exec) = section.attr("Exec") {
            package.exec = exec.into();
        }

        if let Some(icon) = section.attr("Icon") {
            // Always treat as logical theme name.
            package.icon.source = IconSource::Path(PathBuf::from(icon));
            package.icon_small.source = IconSource::Path(PathBuf::from(icon));
        }

        Some(package)
    }
}
