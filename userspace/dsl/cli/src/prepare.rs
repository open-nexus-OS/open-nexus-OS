// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The front half `run` and `render` share: load an app directory or a file, compile it, pick
//! the fixture device of a profile, and read the app's locale catalog
//! (`<app>/i18n/<locale>.nxc` or `.json`) chained onto the pseudo-locale.

use crate::{i18n_cmd, load_input, report};
use nexus_dsl_core::{check_file, has_errors, lower_file};
use std::path::Path;
use std::process::ExitCode;

/// A compiled program, ready to mount.
pub(crate) struct Prepared {
    pub(crate) nxir: Vec<u8>,
    pub(crate) program_hash: String,
    pub(crate) key_names: Vec<String>,
    pub(crate) catalogs: Vec<nexus_dsl_runtime::Catalog>,
    pub(crate) device: nexus_dsl_runtime::FixtureEnv,
}

/// Compiles `path` and gathers what a mount needs; diagnostics are reported on the way.
pub(crate) fn prepare(
    verb: &str,
    path: &str,
    locale_name: Option<&str>,
    profile: &str,
) -> Result<Prepared, ExitCode> {
    let (file, canonical, source) = load_input(path)?;
    let (model, diags) = check_file(&file);
    for diag in &diags {
        report(path, &source, diag);
    }
    if has_errors(&diags) {
        return Err(ExitCode::from(1));
    }
    let lowered = lower_file(&file, &model, &canonical).map_err(|diag| {
        report(path, &source, &diag);
        ExitCode::from(1)
    })?;

    let device = match profile {
        "desktop" => nexus_dsl_runtime::FixtureEnv::desktop(),
        "phone" => nexus_dsl_runtime::FixtureEnv::phone("portrait"),
        "tablet" => nexus_dsl_runtime::FixtureEnv::tablet("landscape"),
        "tv" => {
            let mut env = nexus_dsl_runtime::FixtureEnv::desktop();
            env.profile = "tv";
            env.shell_mode = "tv";
            env.input = &["remote"];
            env
        }
        other => {
            eprintln!("nx-dsl {verb}: unknown profile `{other}`");
            return Err(ExitCode::from(2));
        }
    };

    // Program key names for locale sources.
    let reader =
        nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&lowered.nxir).map_err(|e| {
            eprintln!("nx-dsl {verb}: self-read failed: {e:?}");
            ExitCode::from(1)
        })?;
    let Ok(root) = reader.root() else {
        eprintln!("nx-dsl {verb}: self-read failed");
        return Err(ExitCode::from(1));
    };
    let symbols: Vec<String> = root
        .get_symbols()
        .map(|list| {
            list.iter()
                .map(|s| s.ok().and_then(|t| t.to_str().ok()).map(String::from).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    let key_names = nexus_dsl_runtime::i18n::key_names(root, &symbols);
    let key_refs: Vec<&str> = key_names.iter().map(String::as_str).collect();

    let mut catalogs: Vec<nexus_dsl_runtime::Catalog> = Vec::new();
    if let Some(locale) = locale_name {
        let base = Path::new(path).join("i18n");
        let json = base.join(format!("{locale}.json"));
        let nxc = base.join(format!("{locale}.nxc"));
        let catalog = if nxc.exists() {
            std::fs::read(&nxc)
                .ok()
                .and_then(|b| nexus_dsl_runtime::Catalog::from_binary(&key_refs, &b))
        } else {
            std::fs::read_to_string(&json).ok().and_then(|text| {
                let entries = i18n_cmd::parse_flat_json(&text)?;
                let refs: Vec<(&str, &str)> =
                    entries.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                Some(nexus_dsl_runtime::Catalog::from_entries(&key_refs, &refs))
            })
        };
        match catalog {
            Some(c) => catalogs.push(c),
            None => {
                eprintln!(
                    "nx-dsl {verb}: no readable catalog for locale `{locale}` under `{}`",
                    base.display()
                );
                return Err(ExitCode::from(1));
            }
        }
    }
    Ok(Prepared {
        program_hash: crate::hex(&lowered.program_hash),
        nxir: lowered.nxir,
        key_names,
        catalogs,
        device,
    })
}
