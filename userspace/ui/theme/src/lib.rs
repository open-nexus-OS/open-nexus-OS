// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![allow(clippy::all, warnings)]

//! CONTEXT: Theme token engine for TASK-0057 / RFC-0056.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See tests/ directory
//!
//! PUBLIC API:
//!   - `ThemeRuntime`: loads themes from directory, resolves tokens by qualifier chain.
//!   - `Theme`: parsed .nxtheme.toml with tokens and materials.
//!   - `ColorValue`: RGBA8 color.
//!   - `Qualifier`: theme variant selector (Base, Dark, Light, HighContrast).
//!
//! DEPENDENCIES:
//!   - `toml`: TOML parsing (host-first; OS path will use pre-baked token maps).
//!   - `thiserror`: error derivation.
//!
//! ADR: docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md

pub mod error;
pub mod parser;
pub mod qualifier;
pub mod registry;
pub mod schema;
pub mod tokens;

pub use error::{ThemeError, ThemeResult};
pub use parser::parse_theme_file;
pub use qualifier::Qualifier;
pub use registry::ThemeRegistry;
pub use tokens::{ColorValue, Material, ScaleMap, TokenMap};

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Runtime theme resolver. Loads .nxtheme.toml files from a directory
/// and resolves semantic tokens through the qualifier chain.
#[derive(Debug)]
pub struct ThemeRuntime {
    themes: HashMap<Qualifier, Theme>,
    active: Qualifier,
}

/// The numeric scale sections parsed from `.nxtheme.toml` (whole-number maps).
/// `leading` is line-height ×100 (150 = 1.50); `typography` is font size px;
/// `zindex` is layer order; `motion` is duration ms. Theme-invariant in
/// practice (authored in base; a reduced-motion theme may zero `motion`).
pub const SCALE_SECTIONS: &[&str] =
    &["spacing", "radius", "typography", "leading", "zindex", "motion"];

/// A loaded theme with resolved token map, materials, and numeric scales.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub version: u32,
    pub tokens: TokenMap,
    pub materials: HashMap<String, Material>,
    /// Numeric scale sections keyed by section name (see [`SCALE_SECTIONS`]).
    pub scales: HashMap<String, ScaleMap>,
    /// `[icons]` — the MAINTAINED icon-set linkage: `path` = repo-relative
    /// directory of the SVG set (e.g. the vendored lucide repo), plus the
    /// `[icons.symbols]` map of OUR SwiftUI-style symbol names → file stems.
    /// The icon build imports exactly these (curated, licence-clean).
    pub icons_path: Option<String>,
    pub icon_symbols: Vec<(String, String)>,
}

impl ThemeRuntime {
    /// Load all .nxtheme.toml files from the given directory.
    /// Expects files named like `base.nxtheme.toml`, `dark.nxtheme.toml`, etc.
    pub fn load(theme_dir: &Path) -> ThemeResult<Self> {
        let mut themes = HashMap::new();

        let entries = fs::read_dir(theme_dir)
            .map_err(|e| ThemeError::Io { path: theme_dir.to_path_buf(), source: e })?;

        for entry in entries {
            let entry =
                entry.map_err(|e| ThemeError::Io { path: theme_dir.to_path_buf(), source: e })?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let qualifier = match file_name.strip_suffix(".nxtheme") {
                Some("base") => Qualifier::Base,
                Some("dark") => Qualifier::Dark,
                Some("light") => Qualifier::Light,
                Some("highcontrast") => Qualifier::HighContrast,
                _ => continue,
            };

            let content = fs::read_to_string(&path)
                .map_err(|e| ThemeError::Io { path: path.clone(), source: e })?;
            let theme = parse_theme_file(&content, &path)?;
            themes.insert(qualifier, theme);
        }

        // Base theme is required
        if !themes.contains_key(&Qualifier::Base) {
            return Err(ThemeError::MissingBaseTheme { dir: theme_dir.to_path_buf() });
        }

        Ok(ThemeRuntime { themes, active: Qualifier::Base })
    }

    /// Resolve a semantic token name (e.g. "accent", "bg") to its color value.
    /// Follows the qualifier chain: checks active qualifier first, then falls
    /// back to base.
    pub fn resolve(&self, token_name: &str) -> ThemeResult<ColorValue> {
        let chain = self.active.resolution_chain();

        for qualifier in &chain {
            if let Some(theme) = self.themes.get(qualifier) {
                if let Some(color) = theme.tokens.get(token_name) {
                    return Ok(*color);
                }
            }
        }

        Err(ThemeError::TokenNotFound { token: token_name.to_string(), qualifier: self.active })
    }

    /// Resolve a named material (e.g. "glassPanel", "surface") through the
    /// qualifier chain: active qualifier first, then fall back to base — the
    /// same inheritance as [`resolve`](Self::resolve) for color tokens, so a
    /// theme only redefines the materials that differ.
    pub fn resolve_material(&self, material_name: &str) -> Option<&Material> {
        for qualifier in self.active.resolution_chain() {
            if let Some(theme) = self.themes.get(&qualifier) {
                if let Some(material) = theme.materials.get(material_name) {
                    return Some(material);
                }
            }
        }
        None
    }

    /// Resolve a named step from a numeric scale section (e.g. `("radius","medium")`,
    /// `("typography","base")`) through the qualifier chain. `leading` values are
    /// line-height ×100. See [`SCALE_SECTIONS`].
    pub fn resolve_scale(&self, section: &str, name: &str) -> Option<u32> {
        for qualifier in self.active.resolution_chain() {
            if let Some(px) = self
                .themes
                .get(&qualifier)
                .and_then(|t| t.scales.get(section))
                .and_then(|m| m.get(name))
            {
                return Some(px);
            }
        }
        None
    }

    /// Convenience: `[spacing]` step (px).
    pub fn resolve_spacing(&self, name: &str) -> Option<u32> {
        self.resolve_scale("spacing", name)
    }

    /// Convenience: `[radius]` step (px).
    pub fn resolve_radius(&self, name: &str) -> Option<u32> {
        self.resolve_scale("radius", name)
    }

    /// `[icons] path` — the maintained icon-set directory (authored in base;
    /// icons are theme-invariant).
    pub fn icons_path(&self) -> Option<&str> {
        self.themes.get(&Qualifier::Base).and_then(|t| t.icons_path.as_deref())
    }

    /// `[icons.symbols]` — OUR symbol names → file stems (sorted, base theme).
    pub fn icon_symbols(&self) -> &[(String, String)] {
        self.themes.get(&Qualifier::Base).map(|t| t.icon_symbols.as_slice()).unwrap_or(&[])
    }

    /// Get the active qualifier.
    pub fn active_qualifier(&self) -> Qualifier {
        self.active
    }

    /// Set the active qualifier (e.g. switch to dark mode).
    pub fn set_qualifier(&mut self, qualifier: Qualifier) {
        self.active = qualifier;
    }

    /// Get a reference to the theme for a specific qualifier, if loaded.
    pub fn get_theme(&self, qualifier: Qualifier) -> Option<&Theme> {
        self.themes.get(&qualifier)
    }
}
