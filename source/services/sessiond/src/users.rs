// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sessiond user registry — manifest-driven (TASK-0065B), never
//! hardcoded in a renderer. `[user.<id>]` sections in `manifests/users.toml`;
//! the section suffix is the user id. Mirrors the SystemUI manifest pattern
//! (embedded TOML + mini parser + strict validation, no toml crate).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cargo test -p sessiond`
//! INVARIANTS:
//! - at least one user; ids unique and non-empty; display_name/product non-empty
//! - `auto_login` (optional, `[session]`) must name a registered user
//! - `product` is an opaque SystemUI product id here — resolution is UI policy

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The shipped user registry manifest.
pub const USERS_TOML: &str = include_str!("../manifests/users.toml");

/// One registered user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserEntry {
    /// Stable user id (the `[user.<id>]` section suffix).
    pub id: String,
    /// Name shown on the greeter.
    pub display_name: String,
    /// SystemUI product id selecting this user's shell.
    pub product: String,
}

/// Parsed registry: users + optional auto-login target (index into `users`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRegistry {
    /// Registered users, manifest order.
    pub users: Vec<UserEntry>,
    /// Index of the auto-login user, when configured.
    pub auto_login: Option<usize>,
}

/// Manifest rejection reasons (deterministic, actionable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsersError {
    /// Syntax error or empty/missing required value.
    InvalidManifest,
    /// No `[user.<id>]` section present.
    NoUsers,
    /// Two sections share one user id.
    DuplicateId,
    /// `auto_login` names an id that is not registered.
    UnknownAutoLogin,
}

impl UserRegistry {
    /// Index of a user by id.
    pub fn find(&self, id: &str) -> Option<usize> {
        self.users.iter().position(|u| u.id == id)
    }
}

/// Parses and validates the shipped manifest.
pub fn shipped_registry() -> Result<UserRegistry, UsersError> {
    parse_users_manifest(USERS_TOML)
}

/// Parses a user-registry manifest (see [`USERS_TOML`] for the format).
pub fn parse_users_manifest(input: &str) -> Result<UserRegistry, UsersError> {
    let mut users: Vec<UserEntry> = Vec::new();
    let mut auto_login_id: Option<String> = None;
    let mut section = String::new();
    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.len() <= 2 {
                return Err(UsersError::InvalidManifest);
            }
            section = line[1..line.len() - 1].trim().to_string();
            if section.is_empty() {
                return Err(UsersError::InvalidManifest);
            }
            if let Some(id) = section.strip_prefix("user.") {
                if id.is_empty() {
                    return Err(UsersError::InvalidManifest);
                }
                if users.iter().any(|u| u.id == id) {
                    return Err(UsersError::DuplicateId);
                }
                users.push(UserEntry {
                    id: id.to_string(),
                    display_name: String::new(),
                    product: String::new(),
                });
            }
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some(pair) => pair,
            None => return Err(UsersError::InvalidManifest),
        };
        let key = key.trim();
        let value = unquote(value.trim())?;
        if section == "session" {
            if key == "auto_login" {
                auto_login_id = Some(value);
            }
            continue;
        }
        if section.starts_with("user.") {
            let user = match users.last_mut() {
                Some(user) => user,
                None => return Err(UsersError::InvalidManifest),
            };
            match key {
                "display_name" => user.display_name = value,
                "product" => user.product = value,
                _ => return Err(UsersError::InvalidManifest),
            }
        }
    }
    if users.is_empty() {
        return Err(UsersError::NoUsers);
    }
    if users.iter().any(|u| u.display_name.is_empty() || u.product.is_empty()) {
        return Err(UsersError::InvalidManifest);
    }
    let auto_login = match auto_login_id {
        Some(id) => Some(
            users
                .iter()
                .position(|u| u.id == id)
                .ok_or(UsersError::UnknownAutoLogin)?,
        ),
        None => None,
    };
    Ok(UserRegistry { users, auto_login })
}

/// Strips the surrounding quotes of a TOML string value; rejects anything
/// unquoted, empty, or containing an embedded quote.
fn unquote(value: &str) -> Result<String, UsersError> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(UsersError::InvalidManifest);
    }
    let inner = &value[1..value.len() - 1];
    if inner.is_empty() || inner.contains('"') {
        return Err(UsersError::InvalidManifest);
    }
    Ok(inner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_manifest_parses_and_auto_login_resolves() {
        let registry = shipped_registry().expect("shipped users.toml parses");
        assert!(!registry.users.is_empty());
        assert_eq!(registry.users[0].id, "jenning");
        assert_eq!(registry.users[0].display_name, "Jenning");
        assert_eq!(registry.users[0].product, "default");
        // Phase 1 ships auto-login; the greeter phase drops the line.
        assert_eq!(registry.auto_login, Some(0));
        assert_eq!(registry.find("jenning"), Some(0));
        assert_eq!(registry.find("nobody"), None);
    }

    #[test]
    fn duplicate_id_rejected() {
        let manifest = r#"
[user.a]
display_name = "A"
product = "default"
[user.a]
display_name = "A2"
product = "default"
"#;
        assert_eq!(parse_users_manifest(manifest), Err(UsersError::DuplicateId));
    }

    #[test]
    fn missing_fields_rejected() {
        let manifest = r#"
[user.a]
display_name = "A"
"#;
        assert_eq!(parse_users_manifest(manifest), Err(UsersError::InvalidManifest));
        assert_eq!(parse_users_manifest("[session]\n"), Err(UsersError::NoUsers));
    }

    #[test]
    fn unknown_auto_login_rejected() {
        let manifest = r#"
[session]
auto_login = "ghost"
[user.a]
display_name = "A"
product = "default"
"#;
        assert_eq!(
            parse_users_manifest(manifest),
            Err(UsersError::UnknownAutoLogin)
        );
    }

    #[test]
    fn greeter_mode_when_auto_login_absent() {
        let manifest = r#"
[user.a]
display_name = "A"
product = "default"
"#;
        let registry = parse_users_manifest(manifest).expect("parses");
        assert_eq!(registry.auto_login, None);
    }
}
