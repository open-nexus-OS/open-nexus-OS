// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The validated scenario as JSON for the runner (TASK-0333 6f-3, ADR-0089) —
//! `nexus-proof-manifest scenario` prints it, `tools/qmp_scenario.py` reads it; fields in a
//! deterministic order, strings escaped. Split out of `scenario.rs` (structure gate): the
//! schema and its rules there, the rendering here.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable (the scenario schema grows with the lanes)
//! TEST_COVERAGE: `tests/scenarios.rs` (`a_valid_scenario_resolves_its_points_and_prints_json`)

use super::{Action, Scenario};

impl Scenario {
    /// The scenario as JSON for the runner (deterministic field order).
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\"name\":");
        json_str(&mut out, &self.name);
        out.push_str(",\"profile\":");
        json_str(&mut out, &self.profile);
        out.push_str(",\"start_after\":");
        json_str(&mut out, &self.start_after);
        out.push_str(",\"steps\":[");
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json_str(&mut out, &step.name);
            out.push_str(",\"action\":");
            action_json(&mut out, &step.action);
            out.push_str(",\"expect\":");
            json_list(&mut out, &step.expect);
            out.push_str(",\"then\":");
            json_list(&mut out, &step.then);
            out.push_str(&format!(
                ",\"timeout_s\":{},\"settle_s\":{},\"retry\":{},\"acked_by\":",
                step.timeout_s, step.settle_s, step.retry
            ));
            match &step.acked_by {
                Some(by) => json_str(&mut out, by),
                None => out.push_str("null"),
            }
            out.push_str(",\"retry_with\":[");
            for (j, action) in step.retry_with.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                action_json(&mut out, action);
            }
            out.push_str("]}");
        }
        out.push_str("]}");
        out
    }
}

fn action_json(out: &mut String, action: &Action) {
    match action {
        Action::Login => out.push_str("{\"kind\":\"login\"}"),
        Action::Wait => out.push_str("{\"kind\":\"wait\"}"),
        Action::Move { target, at } | Action::Click { target, at } => {
            let kind = if matches!(action, Action::Move { .. }) { "move" } else { "click" };
            out.push_str(&format!("{{\"kind\":\"{kind}\",\"target\":"));
            json_str(out, target);
            out.push_str(&format!(",\"at\":[{},{}]}}", at.0, at.1));
        }
        Action::Chord { keys } => {
            out.push_str("{\"kind\":\"chord\",\"keys\":");
            json_list(out, keys);
            out.push('}');
        }
        Action::Key { key } => {
            out.push_str("{\"kind\":\"key\",\"key\":");
            json_str(out, key);
            out.push('}');
        }
        Action::Type { word, text, pace_ms } => {
            out.push_str("{\"kind\":\"type\",\"word\":");
            json_str(out, word);
            out.push_str(",\"text\":");
            json_str(out, text);
            if let Some(ms) = pace_ms {
                out.push_str(&format!(",\"pace_ms\":{ms}"));
            }
            out.push('}');
        }
        Action::Drag { from, to } => {
            out.push_str("{\"kind\":\"drag\",\"from\":");
            json_str(out, &from.0);
            out.push_str(&format!(",\"from_at\":[{},{}],\"to\":", from.1 .0, from.1 .1));
            json_str(out, &to.0);
            out.push_str(&format!(",\"to_at\":[{},{}]}}", to.1 .0, to.1 .1));
        }
    }
}

fn json_list(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(out, item);
    }
    out.push(']');
}

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
