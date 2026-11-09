// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::Path;

use crate::error::ThemeError;
use crate::tokens::{Backdrop, ColorValue, ScaleMap, ShadowSpec, TextStyleSpec, TokenMap};
use crate::Theme;

/// Parse a .nxtheme.toml file into a `Theme`.
pub fn parse_theme_file(content: &str, path: &Path) -> Result<Theme, ThemeError> {
    let root: toml::Table = toml::from_str(content)
        .map_err(|e| ThemeError::Parse { path: path.to_path_buf(), message: e.to_string() })?;

    // Collect all top-level section names for validation
    let mut sections: HashSet<String> = HashSet::new();
    for key in root.keys() {
        sections.insert(key.clone());
    }

    crate::schema::validate_sections(&sections, path)?;

    // Parse [theme] section
    let theme_table = root.get("theme").and_then(|v| v.as_table()).ok_or_else(|| {
        ThemeError::MissingSection { section: "theme".to_string(), path: path.to_path_buf() }
    })?;

    crate::schema::validate_theme_section(theme_table, path)?;

    let theme_name = theme_table["name"].as_str().unwrap_or("unknown").to_string();
    let theme_version = theme_table["version"].as_integer().unwrap_or(0) as u32;

    // Parse [tokens] section
    let tokens = if let Some(tokens_value) = root.get("tokens") {
        let tokens_table = tokens_value.as_table().ok_or_else(|| ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: "[tokens] must be a TOML table".to_string(),
        })?;
        crate::schema::validate_tokens_section(tokens_table, path)?;
        parse_token_map(tokens_table, path)?
    } else {
        TokenMap::new()
    };

    // `[material.<name>]` backdrops, `[textStyle.<name>]` and `[shadow.<name>]`: TOML dotted
    // keys create nested tables (root["material"]["regular"]).
    let mut materials = std::collections::HashMap::new();
    for (name, table) in sub_tables(&root, "material", path)? {
        crate::schema::validate_material_section(&name, table, path)?;
        materials.insert(name, parse_backdrop(table));
    }
    let mut text_styles = std::collections::HashMap::new();
    for (name, table) in sub_tables(&root, "textStyle", path)? {
        crate::schema::validate_text_style_section(&name, table, path)?;
        text_styles.insert(name, parse_text_style(table));
    }
    let mut shadows = std::collections::HashMap::new();
    for (name, table) in sub_tables(&root, "shadow", path)? {
        crate::schema::validate_shadow_section(&name, table, path)?;
        shadows.insert(name, parse_shadow(table)?);
    }

    // The numeric scale sections (theme-invariant in practice): whole-number maps keyed by
    // section name — px for spacing, radius and control, layer order for zindex, ms for motion.
    // Only present sections are stored; resolution falls back through the qualifier chain.
    let mut scale_map = std::collections::HashMap::new();
    for section in crate::SCALE_SECTIONS {
        let parsed = parse_scale_section(&root, section, path)?;
        if !parsed.is_empty() {
            scale_map.insert((*section).to_string(), parsed);
        }
    }

    // `[icons]`: path + `[icons.symbols]` (both optional; strings only).
    let mut icons_path = None;
    let mut icon_symbols = Vec::new();
    if let Some(icons) = root.get("icons").and_then(|v| v.as_table()) {
        icons_path = icons.get("path").and_then(|v| v.as_str()).map(String::from);
        if let Some(symbols) = icons.get("symbols").and_then(|v| v.as_table()) {
            for (name, file) in symbols {
                if let Some(file) = file.as_str() {
                    icon_symbols.push((name.clone(), String::from(file)));
                }
            }
        }
        icon_symbols.sort();
    }

    Ok(Theme {
        name: theme_name,
        version: theme_version,
        tokens,
        materials,
        text_styles,
        shadows,
        scales: scale_map,
        icons_path,
        icon_symbols,
    })
}

/// Parse a `[spacing]` / `[radius]` section into a [`ScaleMap`]; empty when absent.
fn parse_scale_section(
    root: &toml::Table,
    section: &str,
    path: &Path,
) -> Result<ScaleMap, ThemeError> {
    let Some(value) = root.get(section) else {
        return Ok(ScaleMap::new());
    };
    let table = value.as_table().ok_or_else(|| ThemeError::SchemaValidation {
        path: path.to_path_buf(),
        message: format!("[{section}] must be a TOML table"),
    })?;
    crate::schema::validate_scale_section(section, table, path)?;
    let mut map = ScaleMap::new();
    for (key, v) in table {
        // validated as non-negative integer above.
        map.insert(key.clone(), v.as_integer().unwrap_or(0) as u32);
    }
    Ok(map)
}

fn parse_token_map(table: &toml::Table, path: &Path) -> Result<TokenMap, ThemeError> {
    let mut map = TokenMap::new();
    for (key, value) in table {
        let hex_str = value.as_str().ok_or_else(|| ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("token '{key}' value must be a string"),
        })?;
        let color = ColorValue::from_hex(hex_str).map_err(|e| {
            // Preserve the path context
            match e {
                ThemeError::InvalidColor { value, reason } => {
                    ThemeError::InvalidColor { value, reason }
                }
                other => other,
            }
        })?;
        map.insert(key.clone(), color);
    }
    Ok(map)
}

/// The `[section.<name>]` sub-tables of `section` (empty when absent).
fn sub_tables<'a>(
    root: &'a toml::Table,
    section: &str,
    path: &Path,
) -> Result<Vec<(String, &'a toml::Table)>, ThemeError> {
    let Some(parent) = root.get(section) else { return Ok(Vec::new()) };
    let parent = parent.as_table().ok_or_else(|| ThemeError::SchemaValidation {
        path: path.to_path_buf(),
        message: format!("[{section}] must hold [{section}.<name>] tables"),
    })?;
    let mut out = Vec::new();
    for (name, value) in parent {
        let table = value.as_table().ok_or_else(|| ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("[{section}.{name}] must be a TOML table"),
        })?;
        out.push((name.clone(), table));
    }
    Ok(out)
}

/// An integer key the schema has already checked.
fn int(table: &toml::Table, key: &str) -> i64 {
    table.get(key).and_then(|v| v.as_integer()).unwrap_or(0)
}

fn parse_backdrop(table: &toml::Table) -> Backdrop {
    Backdrop {
        blur_radius_dp: int(table, "blurRadiusDp") as u32,
        saturation_pct: int(table, "saturationPct") as u32,
        downsample_factor: int(table, "downsampleFactor") as u32,
    }
}

fn parse_text_style(table: &toml::Table) -> TextStyleSpec {
    TextStyleSpec {
        size: int(table, "size") as u32,
        line: int(table, "line") as u32,
        weight: int(table, "weight") as u32,
        tracking_milli_em: int(table, "trackingMilliEm") as i32,
        upper: table.get("case").and_then(|v| v.as_str()) == Some("upper"),
        tabular: table.get("figures").and_then(|v| v.as_str()) == Some("tabular"),
    }
}

fn parse_shadow(table: &toml::Table) -> Result<ShadowSpec, ThemeError> {
    let color = ColorValue::from_hex(table.get("color").and_then(|v| v.as_str()).unwrap_or(""))?;
    Ok(ShadowSpec {
        x: int(table, "x") as i32,
        y: int(table, "y") as i32,
        blur: int(table, "blur") as u32,
        spread: int(table, "spread") as i32,
        color,
        inset: table.get("inset").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}
