// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the arguments of `nx recovery token …` (`.nxra` break-glass tokens, RFC-0088). Split
//! out of `cli.rs` (the module-size ratchet); `cli` re-exports them, so the paths stay.
//! OWNERS: @tools
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `commands/recovery.rs`

use clap::{Args, Subcommand};
use std::path::PathBuf;

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
