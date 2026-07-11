// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::Path;

use crate::error::ThemeError;

/// Known top-level sections in `.nxtheme.toml`.
const KNOWN_SECTIONS: &[&str] = &[
    "theme", "tokens", "material", "spacing", "radius", "typography", "leading", "zindex",
    "motion", "icons",
];

/// Known keys in the `[theme]` section.
const KNOWN_THEME_KEYS: &[&str] = &["name", "version"];

/// Valid material type values.
const KNOWN_MATERIAL_TYPES: &[&str] = &["opaque", "glass"];

/// Validate that all top-level sections in the TOML are known.
pub fn validate_sections(sections: &HashSet<String>, path: &Path) -> Result<(), ThemeError> {
    for section in sections {
        // Material subsections like `material.surface` or `material.glassLow`
        // are checked separately; here we only validate the top-level prefix.
        let top = section.split('.').next().unwrap_or(&section);
        if !KNOWN_SECTIONS.contains(&top) {
            return Err(ThemeError::UnknownSection {
                section: section.clone(),
                path: path.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// Validate the `[theme]` section.
pub fn validate_theme_section(table: &toml::Table, path: &Path) -> Result<(), ThemeError> {
    // Check required keys
    for key in ["name", "version"] {
        if !table.contains_key(key) {
            return Err(ThemeError::MissingSection {
                section: format!("theme.{key}"),
                path: path.to_path_buf(),
            });
        }
    }

    // Check for unknown keys
    let known: HashSet<&str> = KNOWN_THEME_KEYS.iter().copied().collect();
    for key in table.keys() {
        if !known.contains(key.as_str()) {
            return Err(ThemeError::UnknownKey { key: key.clone(), path: path.to_path_buf() });
        }
    }

    // Validate version is an integer ≥ 1
    if let Some(version) = table.get("version").and_then(|v| v.as_integer()) {
        if version < 1 {
            return Err(ThemeError::SchemaValidation {
                path: path.to_path_buf(),
                message: format!("theme.version must be >= 1, got {version}"),
            });
        }
    } else {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: "theme.version must be an integer".to_string(),
        });
    }

    // Validate name is a string
    if table.get("name").and_then(|v| v.as_str()).is_none() {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: "theme.name must be a string".to_string(),
        });
    }

    Ok(())
}

/// Validate the `[tokens]` section.
/// Token values must be strings (hex colors).
pub fn validate_tokens_section(table: &toml::Table, path: &Path) -> Result<(), ThemeError> {
    for (key, value) in table {
        match value.as_str() {
            Some(s) => {
                // Validate hex format
                if !s.starts_with('#') {
                    return Err(ThemeError::SchemaValidation {
                        path: path.to_path_buf(),
                        message: format!(
                            "token '{key}' value '{s}' must be a hex color starting with '#'"
                        ),
                    });
                }
            }
            None => {
                return Err(ThemeError::SchemaValidation {
                    path: path.to_path_buf(),
                    message: format!(
                        "token '{key}' value must be a string (hex color), got {value:?}"
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Validate a length-scale section (`[spacing]` / `[radius]`).
/// Every value must be a non-negative integer number of layout pixels.
pub fn validate_scale_section(
    section: &str,
    table: &toml::Table,
    path: &Path,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        match value.as_integer() {
            Some(px) if px >= 0 => {}
            Some(px) => {
                return Err(ThemeError::SchemaValidation {
                    path: path.to_path_buf(),
                    message: format!("[{section}] '{key}' must be >= 0, got {px}"),
                });
            }
            None => {
                return Err(ThemeError::SchemaValidation {
                    path: path.to_path_buf(),
                    message: format!("[{section}] '{key}' must be an integer pixel count, got {value:?}"),
                });
            }
        }
    }
    Ok(())
}

/// Validate a `[material.*]` subsection.
pub fn validate_material_section(table: &toml::Table, path: &Path) -> Result<(), ThemeError> {
    // Must have a "type" key
    let material_type =
        table.get("type").and_then(|v| v.as_str()).ok_or_else(|| ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: "material section requires a 'type' key".to_string(),
        })?;

    if !KNOWN_MATERIAL_TYPES.contains(&material_type) {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!(
                "unknown material type '{material_type}'; expected one of: {}",
                KNOWN_MATERIAL_TYPES.join(", ")
            ),
        });
    }

    if material_type == "glass" {
        let required = &[
            "blurRadiusDp",
            "downsampleFactor",
            "tintColor",
            "tintAlpha",
            "edgeHighlightColor",
            "edgeHighlightAlpha",
        ];
        for key in required {
            if !table.contains_key(*key) {
                return Err(ThemeError::SchemaValidation {
                    path: path.to_path_buf(),
                    message: format!("glass material requires key '{key}'"),
                });
            }
        }
    }

    Ok(())
}
