// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `clap` command surface for the canonical host-first `nx` CLI.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 27 unit tests and 11 integration tests in the `nx` crate.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use clap::{Args, Parser, Subcommand, ValueEnum};

pub(crate) use crate::cli_image::{
    ImageAction, ImageArgs, ImageBuildArgs, ImageOtaArgs, ImagePatchArgs, ImageVerifyArgs,
};
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
            Commands::Input(args) => match &args.action {
                InputAction::Layouts(a) => a.json,
                InputAction::Status(a) => a.json,
                InputAction::Proof(a) => a.json,
                InputAction::Devices(a) => a.json,
                InputAction::Keymap(args) => match &args.action {
                    InputKeymapAction::Get(a) => a.json,
                    InputKeymapAction::Set(a) => a.json,
                },
                InputAction::Test(args) => match &args.action {
                    InputTestAction::Type(a) => a.json,
                },
                InputAction::Cursor(a) => a.json,
            },
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
            Commands::Crash(args) => match &args.action {
                CrashAction::Ls(a) => a.json,
                CrashAction::Show(a) => a.json,
                CrashAction::Export(a) => a.json,
                CrashAction::Purge(a) => a.json,
                CrashAction::Grep(a) => a.json,
            },
            Commands::Diagnose(args) => args.json,
            Commands::Recovery(args) => match &args.action {
                RecoveryTokenAction::Make(a) => a.json,
                RecoveryTokenAction::Show(a) => a.json,
            },
            Commands::Image(args) => match &args.action {
                ImageAction::Build(a) => a.json,
                ImageAction::Verify(a) => a.json,
                ImageAction::Patch(a) => a.json,
                ImageAction::Ota(a) => a.json,
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
    Input(InputArgs),
    Doctor(DoctorArgs),
    Dsl(DslArgs),
    Config(ConfigArgs),
    Policy(PolicyArgs),
    Crash(CrashArgs),
    Diagnose(DiagnoseArgs),
    Recovery(RecoveryArgs),
    Image(ImageArgs),
}

/// `nx recovery token …` — `.nxra` break-glass tokens (RFC-0088).
#[derive(Args, Debug)]
pub(crate) struct RecoveryArgs {
    #[command(subcommand)]
    pub(crate) action: RecoveryTokenAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum RecoveryTokenAction {
    /// Sign one token from a 32-byte hex seed file.
    #[command(name = "token-make")]
    Make(TokenMakeArgs),
    /// Decode a token and report the verdict against the baked trust.
    #[command(name = "token-show")]
    Show(TokenShowArgs),
}

#[derive(Args, Debug)]
pub(crate) struct TokenMakeArgs {
    /// Key file: 64 hex chars (Ed25519 seed).
    #[arg(long)]
    pub(crate) key: PathBuf,
    /// Action label: slot-switch | target-set | fsck-repair | reset.
    #[arg(long)]
    pub(crate) action: String,
    /// Per-key monotone sequence (unix time works).
    #[arg(long)]
    pub(crate) seq: u64,
    /// Action argument (target/slot/tries byte), default 0.
    #[arg(long, default_value_t = 0)]
    pub(crate) arg: u64,
    /// Optional validity window (both or neither, ns).
    #[arg(long)]
    pub(crate) not_before: Option<u64>,
    #[arg(long)]
    pub(crate) not_after: Option<u64>,
    /// Output path for the 136-byte token.
    #[arg(short, long, default_value = "token.nxra")]
    pub(crate) out: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct TokenShowArgs {
    pub(crate) path: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

/// `nx diagnose` — ONE deterministic diagnostic bundle from a statefs
/// image (TASK-0051; Keystone Gate 6: nx is the only diagnostics CLI).
#[derive(Args, Debug)]
pub(crate) struct DiagnoseArgs {
    /// statefs journal image (QEMU runs leave it at build/blk.img).
    #[arg(long, default_value = "build/blk.img")]
    pub(crate) image: PathBuf,
    /// Output archive path (plain deterministic ustar).
    #[arg(short, long, default_value = "nx-diagnose.tar")]
    pub(crate) out: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
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
pub(crate) struct InputArgs {
    #[command(subcommand)]
    pub(crate) action: InputAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum InputAction {
    Layouts(InputJsonArgs),
    Status(InputJsonArgs),
    Proof(InputJsonArgs),
    Devices(InputJsonArgs),
    Keymap(InputKeymapArgs),
    Test(InputTestArgs),
    Cursor(InputCursorArgs),
}

#[derive(Args, Debug)]
pub(crate) struct InputJsonArgs {
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct InputKeymapArgs {
    #[command(subcommand)]
    pub(crate) action: InputKeymapAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum InputKeymapAction {
    Get(InputJsonArgs),
    Set(InputKeymapSetArgs),
}

#[derive(Args, Debug)]
pub(crate) struct InputKeymapSetArgs {
    pub(crate) layout: String,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct InputTestArgs {
    #[command(subcommand)]
    pub(crate) action: InputTestAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum InputTestAction {
    Type(InputTypeArgs),
}

#[derive(Args, Debug)]
pub(crate) struct InputTypeArgs {
    pub(crate) text: String,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct InputCursorArgs {
    #[arg(allow_hyphen_values = true)]
    pub(crate) x: i32,
    #[arg(allow_hyphen_values = true)]
    pub(crate) y: i32,
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

#[derive(Args, Debug)]
pub(crate) struct CrashArgs {
    #[command(subcommand)]
    pub(crate) action: CrashAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum CrashAction {
    Ls(CrashLsArgs),
    Show(CrashShowArgs),
    Export(CrashExportArgs),
    Purge(CrashPurgeArgs),
    Grep(CrashGrepArgs),
}

#[derive(Args, Debug)]
pub(crate) struct CrashLsArgs {
    #[arg(long, default_value = "./crash")]
    pub(crate) dir: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CrashShowArgs {
    pub(crate) path: PathBuf,
    /// Optional symbols.nxsym index used to symbolize frames.
    #[arg(long)]
    pub(crate) sym: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CrashExportArgs {
    pub(crate) path: PathBuf,
    /// Output path; `.nxcd.zst` (canonical) or `.nxcd` (uncompressed).
    #[arg(short, long)]
    pub(crate) output: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CrashPurgeArgs {
    #[arg(long, default_value = "./crash")]
    pub(crate) dir: PathBuf,
    /// Byte budget for kept dumps (default: unlimited).
    #[arg(long)]
    pub(crate) max_bytes: Option<u64>,
    /// Count budget for kept dumps (default: unlimited).
    #[arg(long)]
    pub(crate) max_count: Option<usize>,
    /// Plan only; delete nothing.
    #[arg(long)]
    pub(crate) dry_run: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CrashGrepArgs {
    pub(crate) pattern: String,
    #[arg(long, default_value = "./crash")]
    pub(crate) dir: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}
