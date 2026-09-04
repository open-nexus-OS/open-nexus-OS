// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx app …` — app bundle authoring for the system volume
//! (TASK-0321 P5). `compile` turns an app project (`manifest.toml` + `ui/`)
//! into its ui-program payload bytes and the `meta/app.properties` sidecar
//! bundlemgrd's registry reads from the volume; `nxb-pack` then packs the
//! bundle directory (scripts/build.sh).
//! OWNERS: @tools-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/app_cli.rs
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub(crate) struct AppArgs {
    #[command(subcommand)]
    pub(crate) action: AppAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum AppAction {
    /// Compile an app project's UI program (+ registry sidecar).
    Compile(AppCompileArgs),
}

#[derive(Args, Debug)]
pub(crate) struct AppCompileArgs {
    /// App project directory (`manifest.toml`, `ui/…`).
    #[arg(long)]
    pub(crate) app: PathBuf,
    /// Output payload path (the bundle's `payload.elf` for ui-program apps).
    #[arg(long)]
    pub(crate) out: PathBuf,
    /// Output `meta/app.properties` (label / icon / bundle_type) for the
    /// volume registry.
    #[arg(long)]
    pub(crate) meta: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}
