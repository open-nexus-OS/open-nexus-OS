// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::Path;

use crate::error::ThemeError;
use crate::tokens::{ColorValue, GlassMaterial, Material, ScaleMap, TokenMap};
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

    // Parse [material.*] sections.
    // TOML dotted keys like [material.surface] create nested tables:
    //   root["material"]["surface"], not root["material.surface"].
    let mut materials = std::collections::HashMap::new();
    if let Some(material_root) = root.get("material").and_then(|v| v.as_table()) {
        for (material_name, value) in material_root {
            let material_table = value.as_table().ok_or_else(|| ThemeError::SchemaValidation {
                path: path.to_path_buf(),
                message: format!("[material.{material_name}] must be a TOML table"),
            })?;
            crate::schema::validate_material_section(material_table, path)?;
            if let Some(mat) = parse_material(material_table, path)? {
                materials.insert(material_name.clone(), mat);
            }
        }
    }

    // Parse the numeric scale sections (theme-invariant in practice): whole-number
    // maps keyed by section name. `leading` is line-height ×100 (150 = 1.50);
    // `typography` is font size px; `zindex` is layer order. Only present sections
    // are stored; resolution falls back through the qualifier chain.
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

fn parse_material(table: &toml::Table, _path: &Path) -> Result<Option<Material>, ThemeError> {
    let material_type = table["type"].as_str().unwrap_or("opaque");

    match material_type {
        "opaque" => Ok(Some(Material::Opaque)),
        "glass" => {
            let tint_color_str = table["tintColor"].as_str().unwrap_or("#ffffff");
            let edge_color_str = table["edgeHighlightColor"].as_str().unwrap_or("#ffffff");
            let border_color_str = table.get("borderColor").and_then(|v| v.as_str());

            let glass = GlassMaterial {
                blur_radius_dp: table["blurRadiusDp"].as_integer().unwrap_or(8) as u32,
                downsample_factor: table["downsampleFactor"].as_integer().unwrap_or(4) as u32,
                tint_color: ColorValue::from_hex(tint_color_str).unwrap_or(ColorValue {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }),
                tint_alpha: table["tintAlpha"].as_float().unwrap_or(0.3) as f32,
                edge_highlight_color: ColorValue::from_hex(edge_color_str).unwrap_or(ColorValue {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }),
                edge_highlight_alpha: table["edgeHighlightAlpha"].as_float().unwrap_or(0.15) as f32,
                border_color: border_color_str.map(|s| ColorValue::from_hex(s).ok()).flatten(),
                border_alpha: table.get("borderAlpha").and_then(|v| v.as_float()).map(|f| f as f32),
            };
            Ok(Some(Material::Glass(glass)))
        }
        _ => Ok(None),
    }
}
