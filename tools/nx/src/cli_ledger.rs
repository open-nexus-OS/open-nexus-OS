// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `clap` surface of `nx ledger` (RFC-0115): read and write planning state.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub(crate) struct LedgerArgs {
    #[command(subcommand)]
    pub(crate) action: LedgerAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum LedgerAction {
    /// Session start: the active milestone, this week's sprint, the work in progress.
    Brief,
    /// Compact view of tasks, RFCs, tracks, milestones or sprints.
    Show {
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// One line per document, filtered.
    List {
        /// task | rfc | track | milestone | sprint
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        subsystem: Option<String>,
    },
    /// The counted progress table of a milestone (default: the active one), as Markdown.
    Table { milestone: Option<String> },
    /// Set one key (`-` removes it); a task's status change stamps `started`/`done`.
    Set { id: String, key: String, value: String },
    /// Set a task's goal: the metric, where it is today, where it must be, how it is measured.
    Goal { id: String, metric: String, baseline: String, target: String, instrument: String },
    /// Create or change a phase (a user story); `--state building|proving|done` stamps dates.
    Phase {
        id: String,
        phase: String,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        story: Option<String>,
        #[arg(long)]
        gate: Option<String>,
        #[arg(long)]
        metric: Option<String>,
        #[arg(long)]
        evidence: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Create or change a milestone step.
    Step {
        milestone: String,
        key: String,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        story: Option<String>,
        #[arg(long)]
        section: Option<String>,
        /// Comma-separated task ids.
        #[arg(long)]
        tasks: Option<String>,
        #[arg(long)]
        gate: Option<String>,
    },
    /// Put a task's phases into a sprint (default: this week's).
    Plan {
        task: String,
        /// Comma-separated phase ids.
        phases: String,
        #[arg(long)]
        week: Option<String>,
        #[arg(long)]
        p50: Option<String>,
        #[arg(long)]
        p85: Option<String>,
        #[arg(long)]
        sample_size: Option<String>,
    },
    /// Mint a task (next free number) or this week's sprint.
    New {
        #[command(subcommand)]
        kind: NewKind,
    },
    /// The gate: every planning document parses, follows the rules and is canonical.
    Lint {
        /// Render every document canonically first.
        #[arg(long)]
        fix: bool,
        /// Also print the notes (references to ids without a file).
        #[arg(long)]
        notes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum NewKind {
    Task {
        #[arg(long)]
        title: String,
        #[arg(long)]
        subsystem: String,
        /// Comma-separated `@handle`s.
        #[arg(long)]
        owner: String,
    },
    Sprint {
        #[arg(long)]
        week: Option<String>,
        #[arg(long)]
        milestone: String,
        #[arg(long)]
        capacity: String,
        #[arg(long)]
        goal: Option<String>,
    },
}
