// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::Path;

use crate::error::ThemeError;

/// Known top-level sections in `.nxtheme.toml` (RFC-0116). A section outside this list — the
/// retired `[typography]` and `[leading]` among them — fails the load.
const KNOWN_SECTIONS: &[&str] = &[
    "theme",
    "tokens",
    "material",
    "spacing",
    "radius",
    "control",
    "textStyle",
    "shadow",
    "zindex",
    "motion",
    "icons",
];

/// Known keys in the `[theme]` section.
const KNOWN_THEME_KEYS: &[&str] = &["name", "version"];

/// The keys of a `[material.<name>]` backdrop, all required.
const BACKDROP_KEYS: &[&str] = &["type", "blurRadiusDp", "saturationPct", "downsampleFactor"];

/// The keys of a `[textStyle.<name>]`, all required.
const TEXT_STYLE_KEYS: &[&str] = &["size", "line", "weight", "trackingMilliEm", "case", "figures"];

/// The keys of a `[shadow.<name>]`, all required.
const SHADOW_KEYS: &[&str] = &["x", "y", "blur", "spread", "color", "inset"];

/// Validate that all top-level sections in the TOML are known.
pub fn validate_sections(sections: &HashSet<String>, path: &Path) -> Result<(), ThemeError> {
    for section in sections {
        // Sub-tables like `material.regular` or `textStyle.body`
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
                    message: format!(
                        "[{section}] '{key}' must be an integer pixel count, got {value:?}"
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Exactly `keys`, each present: a table with a missing or an unknown key is refused.
fn exact_keys(
    table: &toml::Table,
    keys: &[&str],
    what: &str,
    path: &Path,
) -> Result<(), ThemeError> {
    for key in keys {
        if !table.contains_key(*key) {
            return Err(ThemeError::SchemaValidation {
                path: path.to_path_buf(),
                message: format!("{what} requires key '{key}'"),
            });
        }
    }
    for key in table.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(ThemeError::UnknownKey {
                key: format!("{what}.{key}"),
                path: path.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// An integer in `range`, or a schema error naming `what`.
fn int_in(
    table: &toml::Table,
    key: &str,
    range: core::ops::RangeInclusive<i64>,
    what: &str,
    path: &Path,
) -> Result<i64, ThemeError> {
    match table.get(key).and_then(|v| v.as_integer()) {
        Some(v) if range.contains(&v) => Ok(v),
        other => Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("{what}.{key} must be an integer in {range:?}, got {other:?}"),
        }),
    }
}

/// Validate a `[material.<name>]` backdrop: `type = "backdrop"`, blur, saturation, downsample.
pub fn validate_material_section(
    name: &str,
    table: &toml::Table,
    path: &Path,
) -> Result<(), ThemeError> {
    let what = format!("[material.{name}]");
    exact_keys(table, BACKDROP_KEYS, &what, path)?;
    if table.get("type").and_then(|v| v.as_str()) != Some("backdrop") {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("{what}: type must be \"backdrop\" (colours are roles)"),
        });
    }
    int_in(table, "blurRadiusDp", 0..=128, &what, path)?;
    int_in(table, "saturationPct", 0..=400, &what, path)?;
    int_in(table, "downsampleFactor", 1..=16, &what, path)?;
    Ok(())
}

/// Validate a `[textStyle.<name>]`: size and line in px, weight a multiple of 100 in 300–700,
/// signed tracking in milli-em, case `none` or `upper`, figures `proportional` or `tabular`.
pub fn validate_text_style_section(
    name: &str,
    table: &toml::Table,
    path: &Path,
) -> Result<(), ThemeError> {
    let what = format!("[textStyle.{name}]");
    exact_keys(table, TEXT_STYLE_KEYS, &what, path)?;
    let size = int_in(table, "size", 6..=128, &what, path)?;
    let line = int_in(table, "line", 6..=160, &what, path)?;
    if line < size {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("{what}: line {line} is shorter than size {size}"),
        });
    }
    let weight = int_in(table, "weight", 300..=700, &what, path)?;
    if weight % 100 != 0 {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("{what}.weight must be a multiple of 100, got {weight}"),
        });
    }
    int_in(table, "trackingMilliEm", -200..=200, &what, path)?;
    match table.get("case").and_then(|v| v.as_str()) {
        Some("none" | "upper") => {}
        other => {
            return Err(ThemeError::SchemaValidation {
                path: path.to_path_buf(),
                message: format!("{what}.case must be \"none\" or \"upper\", got {other:?}"),
            })
        }
    }
    match table.get("figures").and_then(|v| v.as_str()) {
        Some("proportional" | "tabular") => Ok(()),
        other => Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!(
                "{what}.figures must be \"proportional\" or \"tabular\", got {other:?}"
            ),
        }),
    }
}

/// Validate a `[shadow.<name>]`: signed offsets and spread, a blur, a hex colour, `inset`.
pub fn validate_shadow_section(
    name: &str,
    table: &toml::Table,
    path: &Path,
) -> Result<(), ThemeError> {
    let what = format!("[shadow.{name}]");
    exact_keys(table, SHADOW_KEYS, &what, path)?;
    int_in(table, "x", -64..=64, &what, path)?;
    int_in(table, "y", -64..=64, &what, path)?;
    int_in(table, "blur", 0..=128, &what, path)?;
    int_in(table, "spread", -64..=64, &what, path)?;
    if !table.get("color").and_then(|v| v.as_str()).is_some_and(|c| c.starts_with('#')) {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("{what}.color must be a hex colour string"),
        });
    }
    if table.get("inset").and_then(|v| v.as_bool()).is_none() {
        return Err(ThemeError::SchemaValidation {
            path: path.to_path_buf(),
            message: format!("{what}.inset must be a boolean"),
        });
    }
    Ok(())
}
