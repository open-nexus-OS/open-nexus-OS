// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `clap` command surface for the canonical host-first `nx` CLI.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "nx")]
#[command(about = "Open Nexus host CLI (v1 production-floor)")]
pub struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

impl Cli {
    pub(crate) fn wants_json(&self) -> bool {
        match &self.command {
            Commands::New(args) => match &args.kind {
                NewKind::Service(a) | NewKind::App(a) | NewKind::Test(a) => a.json,
            },
            Commands::Inspect(args) => match &args.target {
                InspectTarget::Nxb(a) => a.json,
            },
            Commands::Idl(args) => match &args.action {
                IdlAction::List(a) => a.json,
                IdlAction::Check(a) => a.json,
            },
            Commands::Postflight(args) => args.json,
            Commands::Doctor(args) => args.json,
            Commands::Dsl(args) => args.json,
            Commands::Config(args) => match &args.action {
                ConfigAction::Validate(a) => a.json,
                ConfigAction::Effective(a) => a.json,
                ConfigAction::Diff(a) => a.json,
                ConfigAction::Push(a) => a.json,
                ConfigAction::Reload(a) => a.json,
                ConfigAction::Where(a) => a.json,
            },
            Commands::Policy(args) => match &args.action {
                PolicyAction::Validate(a) => a.json,
                PolicyAction::Diff(a) => a.json,
                PolicyAction::Explain(a) => a.json,
                PolicyAction::Mode(a) => a.json,
            },
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    New(NewArgs),
    Inspect(InspectArgs),
    Idl(IdlArgs),
    Postflight(PostflightArgs),
    Doctor(DoctorArgs),
    Dsl(DslArgs),
    Config(ConfigArgs),
    Policy(PolicyArgs),
}

#[derive(Args, Debug)]
pub(crate) struct NewArgs {
    #[command(subcommand)]
    pub(crate) kind: NewKind,
}

#[derive(Subcommand, Debug)]
pub(crate) enum NewKind {
    Service(NewItemArgs),
    App(NewItemArgs),
    Test(NewItemArgs),
}

#[derive(Args, Debug)]
pub(crate) struct NewItemArgs {
    pub(crate) name: String,
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct InspectArgs {
    #[command(subcommand)]
    pub(crate) target: InspectTarget,
}

#[derive(Subcommand, Debug)]
pub(crate) enum InspectTarget {
    Nxb(InspectNxbArgs),
}

#[derive(Args, Debug)]
pub(crate) struct InspectNxbArgs {
    pub(crate) path: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct IdlArgs {
    #[command(subcommand)]
    pub(crate) action: IdlAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum IdlAction {
    List(IdlListArgs),
    Check(IdlCheckArgs),
}

#[derive(Args, Debug)]
pub(crate) struct IdlListArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct IdlCheckArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct PostflightArgs {
    pub(crate) topic: String,
    #[arg(long, default_value_t = 40)]
    pub(crate) tail: usize,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct DoctorArgs {
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct DslArgs {
    pub(crate) action: DslAction,
    #[arg(last = true)]
    pub(crate) args: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(ValueEnum, Clone, Debug)]
pub(crate) enum DslAction {
    Fmt,
    Lint,
    Build,
}

#[derive(Args, Debug)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) action: ConfigAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigAction {
    Validate(ConfigValidateArgs),
    Effective(ConfigEffectiveArgs),
    Diff(ConfigDiffArgs),
    Push(ConfigPushArgs),
    Reload(ConfigReloadArgs),
    Where(ConfigWhereArgs),
}

#[derive(Args, Debug)]
pub(crate) struct ConfigValidateArgs {
    pub(crate) paths: Vec<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ConfigEffectiveArgs {
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ConfigDiffArgs {
    #[arg(long)]
    pub(crate) from: PathBuf,
    #[arg(long)]
    pub(crate) to: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ConfigPushArgs {
    pub(crate) file: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ConfigReloadArgs {
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ConfigWhereArgs {
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct PolicyArgs {
    #[command(subcommand)]
    pub(crate) action: PolicyAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum PolicyAction {
    Validate(PolicyValidateArgs),
    Diff(PolicyDiffArgs),
    Explain(PolicyExplainArgs),
    Mode(PolicyModeArgs),
}

#[derive(Args, Debug)]
pub(crate) struct PolicyValidateArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct PolicyDiffArgs {
    #[arg(long)]
    pub(crate) from: PathBuf,
    #[arg(long)]
    pub(crate) to: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct PolicyExplainArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) subject: String,
    #[arg(long = "cap")]
    pub(crate) caps: Vec<String>,
    #[arg(long, value_enum, default_value_t = PolicyCliMode::Enforce)]
    pub(crate) mode: PolicyCliMode,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct PolicyModeArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long, value_enum)]
    pub(crate) set: PolicyCliMode,
    #[arg(long)]
    pub(crate) observed_version: String,
    #[arg(long)]
    pub(crate) actor_service_id: u64,
    #[arg(long)]
    pub(crate) authorized: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PolicyCliMode {
    Enforce,
    DryRun,
    Learn,
}
