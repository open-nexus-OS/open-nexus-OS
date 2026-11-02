// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The document kinds of RFC-0115, where they live, their keys in canonical order and the
//!          type of every key. Pure data plus the id/date/slug checks the rules use.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Task,
    Rfc,
    Track,
    Milestone,
    Sprint,
    Subsystems,
}

impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Task => "task",
            Kind::Rfc => "rfc",
            Kind::Track => "track",
            Kind::Milestone => "milestone",
            Kind::Sprint => "sprint",
            Kind::Subsystems => "subsystems",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Kind> {
        [Kind::Task, Kind::Rfc, Kind::Track, Kind::Milestone, Kind::Sprint, Kind::Subsystems]
            .into_iter()
            .find(|k| k.label() == s)
    }
}

/// Classify a repo-relative path; returns the kind and the id the file name implies.
pub(crate) fn classify(rel: &str) -> Option<(Kind, String)> {
    let stem = rel.strip_suffix(".md")?;
    if let Some(name) = stem.strip_prefix("tasks/milestones/") {
        return (is_slug(name) && !name.contains('/')).then(|| (Kind::Milestone, name.to_string()));
    }
    if let Some(name) = stem.strip_prefix("tasks/sprints/") {
        return is_week(name).then(|| (Kind::Sprint, name.to_string()));
    }
    if stem == "tasks/SUBSYSTEMS" {
        return Some((Kind::Subsystems, String::from("SUBSYSTEMS")));
    }
    if let Some(name) = stem.strip_prefix("tasks/") {
        if name.contains('/') {
            return None;
        }
        if name.starts_with("TASK-") {
            let id = leading_id(name, is_task_id)?;
            return Some((Kind::Task, id));
        }
        if name.starts_with("TRACK-") && is_track_id(name) {
            return Some((Kind::Track, name.to_string()));
        }
        return None;
    }
    if let Some(name) = stem.strip_prefix("docs/rfcs/") {
        if name.contains('/') || !name.starts_with("RFC-") {
            return None;
        }
        return Some((Kind::Rfc, leading_id(name, is_rfc_id)?));
    }
    None
}

/// `TASK-0003B-dsoftbus-…` → `TASK-0003B`: the longest prefix that is an id and ends at `-` or the end.
fn leading_id(name: &str, ok: fn(&str) -> bool) -> Option<String> {
    let mut best = None;
    for (i, c) in name.char_indices() {
        if c == '-' && ok(&name[..i]) {
            best = Some(name[..i].to_string());
        }
    }
    if ok(name) {
        best = Some(name.to_string());
    }
    best
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Ty {
    Text,
    Slug,
    Date,
    Num,
    Schema,
    Enum(&'static [&'static str]),
    Owners,
    TaskId,
    TaskIds,
    RfcIds,
    TrackIds,
    Slugs,
    StoryRefs,
    PhaseId,
    PhaseIds,
    Week,
    StepKey,
    Map(&'static [Field]),
    Records(&'static [Field]),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Field {
    pub(crate) key: &'static str,
    pub(crate) ty: Ty,
    pub(crate) required: bool,
}

const fn f(key: &'static str, ty: Ty, required: bool) -> Field {
    Field { key, ty, required }
}

pub(crate) const TASK_STATUS: &[&str] = &["draft", "in_progress", "done", "superseded", "deferred"];
pub(crate) const RFC_STATUS: &[&str] =
    &["draft", "proposed", "accepted", "in_progress", "done", "superseded"];
pub(crate) const TRACK_STATUS: &[&str] = &["draft", "living", "active", "done", "superseded"];
pub(crate) const MILESTONE_STATUS: &[&str] = &["planned", "active", "done"];
pub(crate) const STEP_STATE: &[&str] = &["planned", "active", "done", "dropped"];
pub(crate) const PHASE_STATE: &[&str] =
    &["planned", "building", "proving", "done", "blocked", "dropped"];
pub(crate) const KIND: &[&str] = &[
    "driver",
    "protocol",
    "service",
    "ui",
    "refactor",
    "bring-up",
    "hardening",
    "contract",
    "tooling",
    "docs",
    "other",
];
pub(crate) const NOVELTY: &[&str] = &["first", "variant", "repeat"];
pub(crate) const DATES_SOURCE: &[&str] = &["recorded", "status-text", "git-reconstructed"];
pub(crate) const TIER: &[&str] = &["production-grade", "production-floor", "beta-floor"];

const LINK: &[Field] = &[f("label", Ty::Text, true), f("target", Ty::Text, true)];
const GOAL: &[Field] = &[
    f("metric", Ty::Text, true),
    f("baseline", Ty::Text, true),
    f("target", Ty::Text, true),
    f("instrument", Ty::Text, true),
    f("result", Ty::Text, false),
];
pub(crate) const PHASE: &[Field] = &[
    f("id", Ty::PhaseId, true),
    f("story", Ty::Text, true),
    f("state", Ty::Enum(PHASE_STATE), true),
    f("gate", Ty::Text, true),
    f("metric", Ty::Text, false),
    f("started", Ty::Date, false),
    f("done", Ty::Date, false),
    f("evidence", Ty::Text, false),
    f("note", Ty::Text, false),
];
const SECTION: &[Field] = &[f("key", Ty::StepKey, true), f("title", Ty::Text, true)];
pub(crate) const STEP: &[Field] = &[
    f("key", Ty::StepKey, true),
    f("section", Ty::StepKey, true),
    f("story", Ty::Text, true),
    f("state", Ty::Enum(STEP_STATE), true),
    f("done", Ty::Date, false),
    f("tasks", Ty::TaskIds, false),
    f("gate", Ty::Text, false),
];
const PLANNED: &[Field] = &[
    f("task", Ty::TaskId, true),
    f("phases", Ty::PhaseIds, true),
    f("p50", Ty::Num, false),
    f("p85", Ty::Num, false),
    f("sample_size", Ty::Num, false),
];
const SUBSYSTEM: &[Field] = &[
    f("slug", Ty::Slug, true),
    f("name", Ty::Text, true),
    f("tier", Ty::Enum(TIER), true),
    f("release", Ty::Text, true),
    f("focus", Ty::Text, false),
];

const TASK: &[Field] = &[
    f("schema", Ty::Schema, true),
    f("id", Ty::Text, true),
    f("title", Ty::Text, true),
    f("status", Ty::Enum(TASK_STATUS), true),
    f("status_note", Ty::Text, false),
    f("owner", Ty::Owners, true),
    f("created", Ty::Date, true),
    f("updated", Ty::Date, false),
    f("subsystem", Ty::Slug, false),
    f("components", Ty::Slugs, false),
    f("kind", Ty::Enum(KIND), false),
    f("novelty", Ty::Enum(NOVELTY), false),
    f("estimate", Ty::Num, false),
    f("started", Ty::Date, false),
    f("done", Ty::Date, false),
    f("active_days", Ty::Num, false),
    f("dates_source", Ty::Enum(DATES_SOURCE), false),
    f("variance_note", Ty::Text, false),
    f("goal", Ty::Map(GOAL), false),
    f("consumer", Ty::Text, false),
    f("depends_on", Ty::TaskIds, false),
    f("follow_ups", Ty::TaskIds, false),
    f("split_from", Ty::TaskId, false),
    f("superseded_by", Ty::TaskIds, false),
    f("implements", Ty::RfcIds, false),
    f("tracks", Ty::TrackIds, false),
    f("links", Ty::Records(LINK), false),
    f("phases", Ty::Records(PHASE), false),
];
const RFC: &[Field] = &[
    f("schema", Ty::Schema, true),
    f("id", Ty::Text, true),
    f("title", Ty::Text, true),
    f("status", Ty::Enum(RFC_STATUS), true),
    f("status_note", Ty::Text, false),
    f("owners", Ty::Owners, true),
    f("created", Ty::Date, true),
    f("updated", Ty::Date, false),
    f("subsystem", Ty::Slug, false),
    f("tasks", Ty::TaskIds, false),
    f("depends_on", Ty::RfcIds, false),
    f("extends", Ty::RfcIds, false),
    f("amends", Ty::RfcIds, false),
    f("superseded_by", Ty::RfcIds, false),
    f("links", Ty::Records(LINK), false),
];
const TRACK: &[Field] = &[
    f("schema", Ty::Schema, true),
    f("id", Ty::Text, true),
    f("title", Ty::Text, true),
    f("status", Ty::Enum(TRACK_STATUS), true),
    f("status_note", Ty::Text, false),
    f("owner", Ty::Owners, true),
    f("created", Ty::Date, true),
    f("updated", Ty::Date, false),
    f("links", Ty::Records(LINK), false),
];
const MILESTONE: &[Field] = &[
    f("schema", Ty::Schema, true),
    f("id", Ty::Slug, true),
    f("title", Ty::Text, true),
    f("status", Ty::Enum(MILESTONE_STATUS), true),
    f("started", Ty::Date, false),
    f("done", Ty::Date, false),
    f("goal", Ty::Text, true),
    f("sections", Ty::Records(SECTION), true),
    f("steps", Ty::Records(STEP), true),
];
const SPRINT: &[Field] = &[
    f("schema", Ty::Schema, true),
    f("id", Ty::Week, true),
    f("milestone", Ty::Slug, true),
    f("capacity_days", Ty::Num, true),
    f("goal", Ty::Text, false),
    f("planned", Ty::Records(PLANNED), true),
    f("completed", Ty::StoryRefs, false),
    f("slipped", Ty::StoryRefs, false),
];
const SUBSYSTEMS: &[Field] =
    &[f("schema", Ty::Schema, true), f("subsystems", Ty::Records(SUBSYSTEM), true)];

pub(crate) fn fields(kind: Kind) -> &'static [Field] {
    match kind {
        Kind::Task => TASK,
        Kind::Rfc => RFC,
        Kind::Track => TRACK,
        Kind::Milestone => MILESTONE,
        Kind::Sprint => SPRINT,
        Kind::Subsystems => SUBSYSTEMS,
    }
}

pub(crate) fn order(spec: &[Field]) -> Vec<&'static str> {
    spec.iter().map(|f| f.key).collect()
}

fn digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn is_task_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("TASK-") else { return false };
    match rest.len() {
        4 => digits(rest, 4),
        5 => digits(&rest[..4], 4) && rest.as_bytes()[4].is_ascii_uppercase(),
        _ => false,
    }
}

pub(crate) fn is_rfc_id(s: &str) -> bool {
    s.strip_prefix("RFC-").is_some_and(|r| digits(r, 4))
}

pub(crate) fn is_track_id(s: &str) -> bool {
    s.strip_prefix("TRACK-").is_some_and(|r| {
        !r.is_empty()
            && r.split('-').all(|p| {
                !p.is_empty() && p.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            })
    })
}

pub(crate) fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|p| {
            !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

pub(crate) fn is_week(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 8 && digits(&s[..4], 4) && b[4] == b'-' && b[5] == b'W' && digits(&s[6..], 2) && {
        let w: u32 = s[6..].parse().unwrap_or(0);
        (1..=53).contains(&w)
    }
}

pub(crate) fn is_phase_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('P') else { return false };
    let digits_end = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits_end > 0
        && digits_end <= 3
        && (rest.len() == digits_end
            || (rest.len() == digits_end + 1 && rest.as_bytes()[digits_end].is_ascii_lowercase()))
}

pub(crate) fn is_step_key(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 12
        && s.as_bytes()[0].is_ascii_uppercase()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

pub(crate) fn is_story_ref(s: &str) -> bool {
    s.split_once('/').is_some_and(|(t, p)| is_task_id(t) && is_phase_id(p))
}

/// `<milestone-slug>/<STEP>` — a milestone step as a consumer.
pub(crate) fn is_step_ref(s: &str) -> bool {
    s.split_once('/').is_some_and(|(m, k)| is_slug(m) && is_step_key(k))
}

/// A consumer the gate can check: a task, a task's phase, or a milestone step.
pub(crate) fn is_consumer_ref(s: &str) -> bool {
    is_task_id(s) || is_story_ref(s) || is_step_ref(s)
}

/// The phase states that count as open work (the brief shows them, the gate wants one).
pub(crate) const OPEN_PHASE: &[&str] = &["building", "proving", "blocked"];

pub(crate) fn is_owner(s: &str) -> bool {
    s.strip_prefix('@').is_some_and(is_slug)
}

pub(crate) fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if !(s.len() == 10
        && digits(&s[..4], 4)
        && b[4] == b'-'
        && digits(&s[5..7], 2)
        && b[7] == b'-'
        && digits(&s[8..], 2))
    {
        return false;
    }
    let m: u32 = s[5..7].parse().unwrap_or(0);
    let d: u32 = s[8..].parse().unwrap_or(0);
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

/// The first day on which RFC-0115's planning fields are required (tasks started on or after it).
pub(crate) const CUTOVER: &str = "2026-11-03";

/// The longest a header status note may be; history goes to the body.
pub(crate) const STATUS_NOTE_MAX: usize = 160;
