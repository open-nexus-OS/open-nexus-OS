// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The visible lanes' choreography as DATA (TASK-0333 6f-3, ADR-0089) — what the host
//! injector does to the guest and what the guest must answer, declared next to the markers it
//! waits for. A scenario (`proof-manifest/scenarios/<name>.toml`) is a list of steps; each step
//! is ONE action (`login`, `click`, `move`, `chord`, `key`, `type`, `drag`, `wait`) and the
//! markers that acknowledge it, within the step's `timeout_s`: `expect` — each a NEW line after
//! the action, in any order (services write concurrently: only causality orders their lines,
//! and a close's `app window closed` and the app's `persist saved` have none between them) —,
//! then `then` — each after the LAST `expect` line, for what follows from them (a re-created
//! surface's first frame after its resize) — or, for an action that writes no line of its own
//! (typing: the keystroke privacy rule), the later step whose acknowledgement covers it
//! (`acked_by`). A step whose acknowledgement does not come is retried (`retry`, optionally with
//! other actions — `retry_with`) or ENDS the scenario with its name and the missing marker: no
//! step's outcome is ever ignored. Nothing is injected before the guest's `start_after` line
//! (the guest says when its timing gates are behind it); a pause that has no event to wait for
//! is declared (`settle_s`), never hidden in code. Display points and typed words live in
//! `scenarios/targets.toml` — the one place the injector and the shell's conformance tests
//! read them from.
//!
//! The contract checked here (host, deterministic): every literal is a declared marker or a
//! specialisation of one (it starts with one, or one starts with it); every target and word
//! exists; every action is one known action; every step but a click or a drag (whose own
//! readback acknowledges them) expects something; a `then` follows an `expect`; names are unique;
//! bounds are bounded.
//! `nexus-proof-manifest scenario` prints the validated scenario as JSON for the runner
//! (`tools/qmp_scenario.py`).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable (the scenario schema grows with the lanes)
//! TEST_COVERAGE: `tests/scenarios.rs` — every shipped scenario valid against the shipped
//!   manifest; `test_reject_*` for an undeclared marker, an unknown target, two actions in one
//!   step, an unacknowledged action, a `then` without an `expect`, a duplicate step, an unknown
//!   profile, out-of-range bounds

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::Manifest;

/// The validated scenario as JSON for the runner.
mod json;

/// The longest a step may wait for its acknowledgement.
pub const MAX_TIMEOUT_S: f64 = 600.0;
/// The longest declared pause.
pub const MAX_SETTLE_S: f64 = 10.0;
/// The most retries a step may declare.
pub const MAX_RETRY: u8 = 3;
/// A step's wait when it declares none.
pub const DEFAULT_TIMEOUT_S: f64 = 30.0;

/// A display-space point (the desktop session shell's pixels).
pub type Point = (i32, i32);

/// The points and words scenarios name (`scenarios/targets.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Targets {
    /// Named display points.
    pub points: BTreeMap<String, Point>,
    /// Named words typed on the keyboard.
    pub words: BTreeMap<String, String>,
}

/// One validated scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    /// The scenario's name (its file stem).
    pub name: String,
    /// The harness profile it drives.
    pub profile: String,
    /// Nothing is injected before this line.
    pub start_after: String,
    /// The steps, in order.
    pub steps: Vec<Step>,
}

/// One step: an action and what acknowledges it.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Unique within the scenario; the failure names it.
    pub name: String,
    /// What the injector does.
    pub action: Action,
    /// New lines after the action, in any order, each within `timeout_s` of it.
    pub expect: Vec<String>,
    /// Lines after the LAST `expect` line, in any order (what follows from them), within the
    /// same bound.
    pub then: Vec<String>,
    /// The bound on the acknowledgement.
    pub timeout_s: f64,
    /// A declared pause BEFORE the action (a settle with no event to wait for).
    pub settle_s: f64,
    /// Retries when the acknowledgement does not come.
    pub retry: u8,
    /// The actions a retry performs (empty: the action again).
    pub retry_with: Vec<Action>,
    /// A step with no line of its own (typing writes none — the keystroke privacy rule) names
    /// the LATER step whose acknowledgement covers it.
    pub acked_by: Option<String>,
}

/// One injector action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The greeter's Submit, read from app-host's handler dump (nothing when already signed in).
    Login,
    /// Travel to a point, closed loop.
    Move {
        /// The target's name.
        target: String,
        /// Its point.
        at: Point,
    },
    /// Travel and click, closed loop; the tap's readback at the point acknowledges it.
    Click {
        /// The target's name.
        target: String,
        /// Its point.
        at: Point,
    },
    /// Modifiers held, the last key tapped, released in reverse.
    Chord {
        /// QEMU qcodes, modifiers first.
        keys: Vec<String>,
    },
    /// One key tapped.
    Key {
        /// A QEMU qcode.
        key: String,
    },
    /// A named word typed letter by letter.
    Type {
        /// The word's name.
        word: String,
        /// Its letters.
        text: String,
    },
    /// Press at `from`, travel with the button held, release at `to`; the drag's trace
    /// acknowledges it.
    Drag {
        /// The press point's name and point.
        from: (String, Point),
        /// The release point's name and point.
        to: (String, Point),
    },
    /// No input: the acknowledgement of what the step before caused, counted from THAT step's
    /// action (a wait has none of its own; counted from its own start it misses a line that came
    /// while the step before was still being acknowledged).
    Wait,
}

/// Why a scenario is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    /// The file could not be read or is not this schema.
    Malformed(String),
    /// The profile is not in the manifest.
    UnknownProfile(String),
    /// A literal no declared marker accounts for.
    UndeclaredMarker {
        /// The step ("start_after" for the head).
        step: String,
        /// The literal.
        literal: String,
    },
    /// A point or word the targets file does not name.
    UnknownTarget {
        /// The step.
        step: String,
        /// The name.
        target: String,
    },
    /// A step's `do` names no action or more than one.
    NotOneAction(String),
    /// A step whose action nothing acknowledges.
    Unacknowledged(String),
    /// Two steps share a name.
    DuplicateStep(String),
    /// A bound outside its range, or a malformed key or word.
    OutOfRange {
        /// The step.
        step: String,
        /// What is wrong.
        what: String,
    },
}

impl core::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "malformed scenario: {why}"),
            Self::UnknownProfile(p) => write!(f, "unknown profile `{p}`"),
            Self::UndeclaredMarker { step, literal } => {
                write!(f, "step `{step}`: `{literal}` is no declared marker")
            }
            Self::UnknownTarget { step, target } => {
                write!(f, "step `{step}`: unknown target `{target}`")
            }
            Self::NotOneAction(step) => {
                write!(f, "step `{step}`: `do` must name exactly one action")
            }
            Self::Unacknowledged(step) => {
                write!(f, "step `{step}`: nothing acknowledges its action")
            }
            Self::DuplicateStep(step) => write!(f, "step `{step}` declared twice"),
            Self::OutOfRange { step, what } => write!(f, "step `{step}`: {what}"),
        }
    }
}

impl std::error::Error for ScenarioError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTargets {
    #[serde(default)]
    targets: BTreeMap<String, [i32; 2]>,
    #[serde(default)]
    words: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenario {
    scenario: RawHead,
    #[serde(default)]
    step: Vec<RawStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHead {
    profile: String,
    start_after: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    name: String,
    #[serde(rename = "do")]
    action: RawAction,
    #[serde(default)]
    expect: Vec<String>,
    #[serde(default)]
    then: Vec<String>,
    timeout_s: Option<f64>,
    #[serde(default)]
    settle_s: f64,
    #[serde(default)]
    retry: u8,
    #[serde(default)]
    retry_with: Vec<RawAction>,
    acked_by: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawAction {
    login: Option<bool>,
    #[serde(rename = "move")]
    move_to: Option<String>,
    click: Option<String>,
    chord: Option<Vec<String>>,
    key: Option<String>,
    #[serde(rename = "type")]
    type_word: Option<String>,
    drag: Option<[String; 2]>,
    wait: Option<bool>,
}

/// Parses `scenarios/targets.toml`.
pub fn parse_targets(src: &str) -> Result<Targets, ScenarioError> {
    let raw: RawTargets =
        toml::from_str(src).map_err(|e| ScenarioError::Malformed(e.to_string()))?;
    Ok(Targets {
        points: raw.targets.into_iter().map(|(k, [x, y])| (k, (x, y))).collect(),
        words: raw.words,
    })
}

/// Reads and parses the targets file at `path`.
pub fn read_targets(path: &Path) -> Result<Targets, ScenarioError> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| ScenarioError::Malformed(format!("{}: {e}", path.display())))?;
    parse_targets(&src)
}

/// Parses and validates the scenario `name` from `src` against `manifest` and `targets`.
pub fn parse(
    name: &str,
    src: &str,
    manifest: &Manifest,
    targets: &Targets,
) -> Result<Scenario, ScenarioError> {
    let raw: RawScenario =
        toml::from_str(src).map_err(|e| ScenarioError::Malformed(e.to_string()))?;
    if !manifest.profiles.contains_key(&raw.scenario.profile) {
        return Err(ScenarioError::UnknownProfile(raw.scenario.profile));
    }
    let declared: Vec<&str> = manifest.markers.iter().map(|m| m.literal.as_str()).collect();
    check_declared(&declared, "start_after", &raw.scenario.start_after)?;
    let mut names = std::collections::BTreeSet::new();
    let mut steps = Vec::with_capacity(raw.step.len());
    for raw_step in raw.step {
        let step = raw_step.name.clone();
        if !names.insert(step.clone()) {
            return Err(ScenarioError::DuplicateStep(step));
        }
        let action = action(&step, raw_step.action, targets)?;
        let retry_with = raw_step
            .retry_with
            .into_iter()
            .map(|a| action_for_retry(&step, a, targets))
            .collect::<Result<Vec<_>, _>>()?;
        for literal in raw_step.expect.iter().chain(&raw_step.then) {
            check_declared(&declared, &step, literal)?;
        }
        if !raw_step.then.is_empty() && raw_step.expect.is_empty() {
            return Err(out_of_range(&step, "then without an expect to follow"));
        }
        let acknowledged_by_readback = matches!(action, Action::Click { .. } | Action::Drag { .. });
        match (&raw_step.acked_by, raw_step.expect.is_empty()) {
            (Some(_), false) => return Err(out_of_range(&step, "acked_by beside its own expect")),
            (None, true) if !acknowledged_by_readback => {
                return Err(ScenarioError::Unacknowledged(step))
            }
            _ => {}
        }
        let timeout_s = raw_step.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S);
        if !(timeout_s > 0.0 && timeout_s <= MAX_TIMEOUT_S) {
            return Err(out_of_range(&step, "timeout_s outside (0, 600]"));
        }
        if !(0.0..=MAX_SETTLE_S).contains(&raw_step.settle_s) {
            return Err(out_of_range(&step, "settle_s outside [0, 10]"));
        }
        if raw_step.retry > MAX_RETRY {
            return Err(out_of_range(&step, "retry above 3"));
        }
        if !retry_with.is_empty() && raw_step.retry == 0 {
            return Err(out_of_range(&step, "retry_with without a retry"));
        }
        steps.push(Step {
            name: step,
            action,
            expect: raw_step.expect,
            then: raw_step.then,
            timeout_s,
            settle_s: raw_step.settle_s,
            retry: raw_step.retry,
            retry_with,
            acked_by: raw_step.acked_by,
        });
    }
    if steps.is_empty() {
        return Err(ScenarioError::Malformed("no step".into()));
    }
    // `acked_by` names a LATER step that expects something.
    for (i, step) in steps.iter().enumerate() {
        let Some(by) = &step.acked_by else { continue };
        let later = steps[i + 1..].iter().any(|s| &s.name == by && !s.expect.is_empty());
        if !later {
            return Err(ScenarioError::Unacknowledged(step.name.clone()));
        }
    }
    Ok(Scenario {
        name: name.to_string(),
        profile: raw.scenario.profile,
        start_after: raw.scenario.start_after,
        steps,
    })
}

/// Reads `<dir>/<name>.toml` and `<dir>/targets.toml` and validates the scenario.
pub fn read(dir: &Path, name: &str, manifest: &Manifest) -> Result<Scenario, ScenarioError> {
    let targets = read_targets(&dir.join("targets.toml"))?;
    let path = dir.join(format!("{name}.toml"));
    let src = std::fs::read_to_string(&path)
        .map_err(|e| ScenarioError::Malformed(format!("{}: {e}", path.display())))?;
    parse(name, &src, manifest, &targets)
}

fn out_of_range(step: &str, what: &str) -> ScenarioError {
    ScenarioError::OutOfRange { step: step.to_string(), what: what.to_string() }
}

/// A literal is accounted for when a declared marker is a prefix of it (a specialisation:
/// `…kill ok (band=suspended reason=oom)`) or it is a prefix of a declared marker.
fn check_declared(declared: &[&str], step: &str, literal: &str) -> Result<(), ScenarioError> {
    let known = !literal.is_empty()
        && declared.iter().any(|d| literal.starts_with(d) || d.starts_with(literal));
    if known {
        Ok(())
    } else {
        Err(ScenarioError::UndeclaredMarker {
            step: step.to_string(),
            literal: literal.to_string(),
        })
    }
}

fn point(step: &str, name: &str, targets: &Targets) -> Result<Point, ScenarioError> {
    targets.points.get(name).copied().ok_or_else(|| ScenarioError::UnknownTarget {
        step: step.to_string(),
        target: name.to_string(),
    })
}

fn qcode(step: &str, key: &str) -> Result<(), ScenarioError> {
    let ok = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(out_of_range(step, &format!("`{key}` is not a qcode")))
    }
}

fn action(step: &str, raw: RawAction, targets: &Targets) -> Result<Action, ScenarioError> {
    let count = [
        raw.login.is_some(),
        raw.move_to.is_some(),
        raw.click.is_some(),
        raw.chord.is_some(),
        raw.key.is_some(),
        raw.type_word.is_some(),
        raw.drag.is_some(),
        raw.wait.is_some(),
    ]
    .iter()
    .filter(|set| **set)
    .count();
    if count != 1 {
        return Err(ScenarioError::NotOneAction(step.to_string()));
    }
    if let Some(on) = raw.login {
        return if on { Ok(Action::Login) } else { Err(ScenarioError::NotOneAction(step.into())) };
    }
    if let Some(on) = raw.wait {
        return if on { Ok(Action::Wait) } else { Err(ScenarioError::NotOneAction(step.into())) };
    }
    if let Some(target) = raw.move_to {
        let at = point(step, &target, targets)?;
        return Ok(Action::Move { target, at });
    }
    if let Some(target) = raw.click {
        let at = point(step, &target, targets)?;
        return Ok(Action::Click { target, at });
    }
    if let Some(keys) = raw.chord {
        if keys.len() < 2 {
            return Err(out_of_range(step, "a chord needs a modifier and a key"));
        }
        for key in &keys {
            qcode(step, key)?;
        }
        return Ok(Action::Chord { keys });
    }
    if let Some(key) = raw.key {
        qcode(step, &key)?;
        return Ok(Action::Key { key });
    }
    if let Some(word) = raw.type_word {
        let text = targets.words.get(&word).cloned().ok_or_else(|| {
            ScenarioError::UnknownTarget { step: step.to_string(), target: word.clone() }
        })?;
        if text.is_empty() || !text.chars().all(|c| c.is_ascii_lowercase()) {
            return Err(out_of_range(step, "a typed word is lowercase letters"));
        }
        return Ok(Action::Type { word, text });
    }
    if let Some([from, to]) = raw.drag {
        let from_at = point(step, &from, targets)?;
        let to_at = point(step, &to, targets)?;
        return Ok(Action::Drag { from: (from, from_at), to: (to, to_at) });
    }
    Err(ScenarioError::NotOneAction(step.to_string()))
}

/// A retry repeats input, never a login or a bare wait.
fn action_for_retry(
    step: &str,
    raw: RawAction,
    targets: &Targets,
) -> Result<Action, ScenarioError> {
    match action(step, raw, targets)? {
        Action::Login | Action::Wait => Err(out_of_range(step, "a retry repeats input only")),
        other => Ok(other),
    }
}
