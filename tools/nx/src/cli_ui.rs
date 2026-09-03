// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `clap` arg structs for `nx ui preset list/env` (TASK-0055D) —
//! the dev-mode display/profile preset resolver's CLI face, split from
//! `cli.rs` under the structure ratchet (same pattern as `cli_image.rs`).
//! OWNERS: @tools-team @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/ui_preset_cli.rs (process boundary)
//! ADR: docs/adr/0035-systemui-declarative-shell-configuration.md

use clap::{Args, Subcommand};

/// `nx ui …` — host-side UI developer tooling.
#[derive(Args, Debug)]
pub(crate) struct UiArgs {
    #[command(subcommand)]
    pub(crate) action: UiAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiAction {
    /// Dev-mode display/profile presets (`just start-preset <name>`).
    Preset(UiPresetArgs),
}

#[derive(Args, Debug)]
pub(crate) struct UiPresetArgs {
    #[command(subcommand)]
    pub(crate) action: UiPresetAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UiPresetAction {
    /// List the registered presets (id, profile, shell, mode).
    List(UiPresetListArgs),
    /// Resolve ONE preset into launcher environment lines (`KEY=VALUE`).
    Env(UiPresetEnvArgs),
}

#[derive(Args, Debug)]
pub(crate) struct UiPresetListArgs {
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct UiPresetEnvArgs {
    /// Preset id (see `nx ui preset list`).
    pub(crate) name: String,
    #[arg(long)]
    pub(crate) json: bool,
}
