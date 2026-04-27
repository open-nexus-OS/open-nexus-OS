// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Command routing for the canonical `nx` CLI.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

pub(crate) mod config;
pub(crate) mod doctor;
pub(crate) mod dsl;
pub(crate) mod idl;
pub(crate) mod inspect;
pub(crate) mod new;
pub(crate) mod policy;
pub(crate) mod postflight;

use crate::cli::{Cli, Commands};
use crate::error::ExecResult;
use crate::output::print_result;
use crate::runtime::RuntimeConfig;
use clap::Parser;

pub fn run() -> i32 {
    let _ = crate::error::ExitClass::Usage.code();
    let cli = Cli::parse();
    let wants_json = cli.wants_json();
    let cfg = match RuntimeConfig::from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            let code = err.class.code();
            print_result(err.class, err.message, wants_json, None);
            return code;
        }
    };
    match execute(cli, &cfg) {
        Ok((class, message, json_mode, data)) => {
            print_result(class, message, json_mode, data);
            class.code()
        }
        Err(err) => {
            let code = err.class.code();
            print_result(err.class, err.message, wants_json, None);
            code
        }
    }
}

pub(crate) fn execute(cli: Cli, cfg: &RuntimeConfig) -> ExecResult {
    match cli.command {
        Commands::New(args) => new::handle_new(args, cfg),
        Commands::Inspect(args) => inspect::handle_inspect(args),
        Commands::Idl(args) => idl::handle_idl(args, cfg),
        Commands::Postflight(args) => postflight::handle_postflight(args, cfg),
        Commands::Doctor(args) => doctor::handle_doctor(args),
        Commands::Dsl(args) => dsl::handle_dsl(args, cfg),
        Commands::Config(args) => config::handle_config(args, cfg),
        Commands::Policy(args) => policy::handle_policy(args, cfg),
    }
}
