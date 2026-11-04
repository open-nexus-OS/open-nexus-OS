// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The board cycles' gate values as a series (ADR-0094, TASK-0351 P8): `nx ledger
//!          series add <uart.log>` reads one board cycle's gate lines — `gpud: gpu gate scene`
//!          (cold, pipelined, hover), `gpud: gpu work` (ns a pixel, the run's median), `gpud: ux
//!          frame` (dropped of frames, the slowest), `gpud: gpu pass sweep` (the 16x16 frame, the
//!          full quad under a 16x16 damage) — into `docs/board/measurements/series.toml`, one
//!          table per cycle. `nx ledger lint --fix` renders two files from it, `lint` holds them
//!          current: the series as a table (`docs/board/measurements/series.md`) and the last
//!          cycle's values as constants the device server prints as `last=` on every gate line
//!          and judges a regression against (`source/drivers/gpud/src/backend/gpu/series.rs`).
//!          "Why is this slower than before" is answered by a cycle id, never by wondering.
//! OWNERS: @tools-team @gpu
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_series.rs; unit tests below
//! ADR: docs/adr/0094-one-kernel-three-executors-an-effect-s-mathematics-is-defined-once-in-nexus-gfx.md

use std::fs;
use std::path::Path;

pub(crate) const SERIES: &str = "docs/board/measurements/series.toml";
pub(crate) const TABLE: &str = "docs/board/measurements/series.md";
pub(crate) const CONSTANTS: &str = "source/drivers/gpud/src/backend/gpu/series.rs";
/// A value worse than the last cycle's by more than this is a regression (the gate names the
/// cycle).
pub(crate) const MARGIN_PERCENT: u64 = 10;

/// The values a cycle recorded; `0` = the line was not in the log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cycle {
    pub(crate) id: String,
    pub(crate) cold_us: u64,
    pub(crate) pipelined_us: u64,
    pub(crate) hover_us: u64,
    pub(crate) ns_per_px: u64,
    pub(crate) ux_dropped: u64,
    pub(crate) ux_frames: u64,
    pub(crate) ux_max_us: u64,
    pub(crate) q16_us: u64,
    pub(crate) area16_us: u64,
}

const FIELDS: [&str; 9] = [
    "cold_us",
    "pipelined_us",
    "hover_us",
    "ns_per_px",
    "ux_dropped",
    "ux_frames",
    "ux_max_us",
    "q16_us",
    "area16_us",
];

impl Cycle {
    fn get(&self, field: &str) -> u64 {
        match field {
            "cold_us" => self.cold_us,
            "pipelined_us" => self.pipelined_us,
            "hover_us" => self.hover_us,
            "ns_per_px" => self.ns_per_px,
            "ux_dropped" => self.ux_dropped,
            "ux_frames" => self.ux_frames,
            "ux_max_us" => self.ux_max_us,
            "q16_us" => self.q16_us,
            "area16_us" => self.area16_us,
            _ => 0,
        }
    }

    fn set(&mut self, field: &str, v: u64) {
        match field {
            "cold_us" => self.cold_us = v,
            "pipelined_us" => self.pipelined_us = v,
            "hover_us" => self.hover_us = v,
            "ns_per_px" => self.ns_per_px = v,
            "ux_dropped" => self.ux_dropped = v,
            "ux_frames" => self.ux_frames = v,
            "ux_max_us" => self.ux_max_us = v,
            "q16_us" => self.q16_us = v,
            "area16_us" => self.area16_us = v,
            _ => {}
        }
    }
}

/// `key=value` numbers inside the parentheses of a line, in order.
fn pairs(line: &str) -> Vec<(&str, u64)> {
    let inner = line.find('(').map_or(line, |i| &line[i + 1..]);
    inner
        .split([' ', ')', ';', ':'])
        .filter_map(|tok| {
            let (k, v) = tok.split_once('=')?;
            let v = v.split(['/', ',']).next()?;
            Some((k, v.parse::<u64>().ok()?))
        })
        .collect()
}

fn value(p: &[(&str, u64)], key: &str) -> Option<u64> {
    p.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// One cycle read from a UART log: `None` when it holds no gate scene line.
pub(crate) fn read_log(id: &str, text: &str) -> Option<Cycle> {
    let mut c = Cycle { id: id.to_string(), ..Cycle::default() };
    let mut ns: Vec<u64> = Vec::new();
    let mut gate = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("gpud: gpu gate scene ok (")
            || line.starts_with("gpud: FAIL gpu gate scene (")
        {
            let p = pairs(line);
            c.cold_us = value(&p, "cold_us").unwrap_or(0);
            c.pipelined_us = value(&p, "pipelined_us").unwrap_or(0);
            c.hover_us = value(&p, "hover_us").unwrap_or(0);
            gate = true;
        } else if line.starts_with("gpud: gpu work (") {
            if let Some(v) = value(&pairs(line), "ns_per_px") {
                ns.push(v);
            }
        } else if line.starts_with("gpud: ux frame ") {
            let p = pairs(line);
            c.ux_dropped += value(&p, "dropped").unwrap_or(0);
            c.ux_frames += value(&p, "of").unwrap_or(0);
            c.ux_max_us = c.ux_max_us.max(value(&p, "max_us").unwrap_or(0));
        } else if line.starts_with("gpud: gpu pass sweep (") {
            let p = pairs(line);
            c.q16_us = value(&p, "q16_us").unwrap_or(0);
            c.area16_us = value(&p, "area16_us").unwrap_or(0);
        }
    }
    if !gate {
        return None;
    }
    ns.sort_unstable();
    c.ns_per_px = ns.get(ns.len() / 2).copied().unwrap_or(0);
    Some(c)
}

/// The cycle id a log path names: its folder `board--<id>`; none for any other folder.
pub(crate) fn id_of(log: &Path) -> Option<String> {
    let dir = log.parent()?.file_name()?.to_str()?;
    dir.strip_prefix("board--").filter(|id| !id.is_empty()).map(str::to_string)
}

/// The series on disk, oldest first; none when the file is absent.
pub(crate) fn load(root: &Path) -> Result<Vec<Cycle>, String> {
    let path = root.join(SERIES);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{SERIES}: {e}"))?;
    let t: toml::Table = text.parse().map_err(|e| format!("{SERIES}: {e}"))?;
    let mut out = Vec::new();
    if let Some(toml::Value::Array(list)) = t.get("cycle") {
        for v in list {
            let toml::Value::Table(c) = v else {
                return Err(format!("{SERIES}: a cycle is not a table"));
            };
            let Some(toml::Value::String(id)) = c.get("id") else {
                return Err(format!("{SERIES}: a cycle without an `id`"));
            };
            let mut cycle = Cycle { id: id.clone(), ..Cycle::default() };
            for f in FIELDS {
                if let Some(toml::Value::Integer(n)) = c.get(f) {
                    cycle.set(f, u64::try_from(*n).unwrap_or(0));
                }
            }
            out.push(cycle);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// The series written back: `cycle` replaces the one with its id, else is added.
pub(crate) fn add(root: &Path, cycle: Cycle) -> Result<usize, String> {
    let mut all = load(root)?;
    all.retain(|c| c.id != cycle.id);
    all.push(cycle);
    all.sort_by(|a, b| a.id.cmp(&b.id));
    let mut text = String::from(
        "# The board cycles' gate values (ADR-0094, TASK-0351 P8): one table per cycle, written by\n# `nx ledger series add <uart.log>`; `nx ledger lint --fix` renders series.md and the device\n# server's `series.rs` (the last cycle as `last=`) from it. Never edited by hand.\n",
    );
    for c in &all {
        text.push_str(&format!("\n[[cycle]]\nid = \"{}\"\n", c.id));
        for f in FIELDS {
            text.push_str(&format!("{f} = {}\n", c.get(f)));
        }
    }
    let path = root.join(SERIES);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{SERIES}: {e}"))?;
    }
    fs::write(&path, text).map_err(|e| format!("{SERIES}: {e}"))?;
    Ok(all.len())
}

/// The series as a markdown table.
pub(crate) fn render_table(all: &[Cycle]) -> String {
    let mut s = String::from(
        "<!-- Copyright 2026 Open Nexus OS Contributors -->\n<!-- SPDX-License-Identifier: Apache-2.0 -->\n<!-- GENERATED by `nx ledger lint --fix` from series.toml (ADR-0094, TASK-0351 P8) — do not edit. -->\n\n# The board cycles' gate values\n\nOne row per board cycle (`nx ledger series add build/logs/board--<id>/uart.log`), newest last. The device server prints the last row as `last=` on its gate lines and a value worse by more than 10 % is `FAIL … slower than last` naming that cycle. Times in microseconds.\n\n| cycle | gate cold | pipelined | hover | ns/px | ux dropped | ux slowest | sweep 16x16 | full quad, 16x16 damage |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    for c in all {
        let n = |v: u64| if v == 0 { "—".to_string() } else { v.to_string() };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {}/{} | {} | {} | {} |\n",
            c.id,
            n(c.cold_us),
            n(c.pipelined_us),
            n(c.hover_us),
            n(c.ns_per_px),
            c.ux_dropped,
            c.ux_frames,
            n(c.ux_max_us),
            n(c.q16_us),
            n(c.area16_us)
        ));
    }
    s
}

/// The last cycle as the device server's constants.
pub(crate) fn render_constants(all: &[Cycle]) -> String {
    let mut s = String::from(
        "// Copyright 2026 Open Nexus OS Contributors\n// SPDX-License-Identifier: Apache-2.0\n// GENERATED by `nx ledger lint --fix` from docs/board/measurements/series.toml — do not edit.\n\n//! The last recorded board cycle's gate values (ADR-0094, TASK-0351 P8): what every gate line\n//! prints as `last=` and judges a regression against — a value worse than the last by more than\n//! `MARGIN_PERCENT` is `FAIL … slower than last` naming the cycle.\n\n/// One cycle's values (microseconds; `0` = not recorded).\n#[derive(Clone, Copy, Debug)]\npub(super) struct Last {\n    pub cycle: &'static str,\n    pub cold_us: u64,\n    pub pipelined_us: u64,\n    pub hover_us: u64,\n    pub ns_per_px: u64,\n    pub ux_max_us: u64,\n    pub q16_us: u64,\n    pub area16_us: u64,\n}\n\n",
    );
    s.push_str(&format!("pub(super) const MARGIN_PERCENT: u64 = {MARGIN_PERCENT};\n\n"));
    match all.last() {
        None => s.push_str("pub(super) const LAST: Option<Last> = None;\n"),
        Some(c) => s.push_str(&format!(
            "pub(super) const LAST: Option<Last> = Some(Last {{\n    cycle: \"{}\",\n    cold_us: {},\n    pipelined_us: {},\n    hover_us: {},\n    ns_per_px: {},\n    ux_max_us: {},\n    q16_us: {},\n    area16_us: {},\n}});\n",
            c.id, c.cold_us, c.pipelined_us, c.hover_us, c.ns_per_px, c.ux_max_us, c.q16_us, c.area16_us
        )),
    }
    s
}

/// The files rendered from the series, repo-relative with their text; none without a series.
pub(crate) fn files(root: &Path) -> Result<Vec<(String, String)>, String> {
    if !root.join(SERIES).exists() {
        return Ok(Vec::new());
    }
    let all = load(root)?;
    Ok(vec![
        (TABLE.to_string(), render_table(&all)),
        (CONSTANTS.to_string(), render_constants(&all)),
    ])
}

/// `nx ledger series add`: the cycle `log` holds recorded under `id` (default: its folder's).
pub(crate) fn add_log(root: &Path, log: &Path, id: Option<&str>) -> Result<String, String> {
    let id = match id {
        Some(id) => id.to_string(),
        None => id_of(log).ok_or_else(|| {
            format!("{}: no `board--<id>` folder names the cycle; pass `--id`", log.display())
        })?,
    };
    if id.is_empty() || id.contains(['"', '\n']) {
        return Err(format!("`{id}` is not a cycle id"));
    }
    let text = fs::read_to_string(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let cycle = read_log(&id, &text)
        .ok_or_else(|| format!("{}: no `gpud: gpu gate scene` line", log.display()))?;
    let n = add(root, cycle)?;
    Ok(format!("recorded cycle {id} ({n} in {SERIES}); `nx ledger lint --fix` renders {TABLE} and {CONSTANTS}\n"))
}

/// The rendered files written where they differ from disk: their paths.
pub(crate) fn write(root: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (rel, text) in files(root)? {
        let path = root.join(&rel);
        if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| format!("{rel}: {e}"))?;
            }
            fs::write(&path, text).map_err(|e| format!("{rel}: {e}"))?;
            out.push(rel);
        }
    }
    Ok(out)
}

/// The gate's problems: a rendered file missing or stale (fatal), or a series that does not
/// parse.
pub(crate) fn problems(root: &Path) -> Vec<super::rules::Problem> {
    let fatal = |rel: &str, message: String| super::rules::Problem {
        rel: rel.to_string(),
        message,
        fatal: true,
    };
    match files(root) {
        Err(e) => vec![fatal(SERIES, e)],
        Ok(files) => files
            .into_iter()
            .filter(|(rel, text)| {
                fs::read_to_string(root.join(rel)).ok().as_deref() != Some(text.as_str())
            })
            .map(|(rel, _)| {
                fatal(
                    &rel,
                    String::from("stale against the series; `nx ledger lint --fix` renders it"),
                )
            })
            .collect(),
    }
}

/// The series as the command prints it.
pub(crate) fn show(root: &Path) -> Result<String, String> {
    let all = load(root)?;
    if all.is_empty() {
        return Ok(format!("no cycles recorded ({SERIES})\n"));
    }
    Ok(render_table(&all))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "gpud: gpu frame us (n=32 avg=1 max=2)\ngpud: FAIL gpu gate scene (cold_us=48505 pipelined_us=35803 bar=23300 hover_us=12739 hover_bar=8300: the scene above its bar; warm_us=12739/16306/13531 passes=4/1 pass_wait_us=15286 iters=11 irqs=4)\ngpud: gpu work (slot_px=1 scene_px=2 blur_px=3 ns_per_px=13 opaque_px=4 straight_px=5 rounded_px=6 shadow_px=7 layers=8)\ngpud: gpu work (slot_px=1 scene_px=2 blur_px=3 ns_per_px=11 opaque_px=4 straight_px=5 rounded_px=6 shadow_px=7 layers=8)\ngpud: gpu work (slot_px=1 scene_px=2 blur_px=3 ns_per_px=17 opaque_px=4 straight_px=5 rounded_px=6 shadow_px=7 layers=8)\ngpud: ux frame FAIL (dropped=3 of=32 max_us=120228 budget_us=16700)\ngpud: ux frame ok (dropped=0 of=32 max_us=9000 budget_us=16700)\ngpud: gpu pass sweep (base_us=700 q16_us=900 q240_us=1000 q480_us=1200 q960_us=2000 full_us=4000 area16_us=3900 blended_us=7000 frames=5)\n";

    #[test]
    fn a_log_s_gate_lines_become_one_cycle() {
        let c = read_log("2026-11-03T19-13-30", LOG).expect("a cycle");
        assert_eq!((c.cold_us, c.pipelined_us, c.hover_us), (48505, 35803, 12739));
        assert_eq!(c.ns_per_px, 13, "the median of 11, 13, 17");
        assert_eq!((c.ux_dropped, c.ux_frames, c.ux_max_us), (3, 64, 120228));
        assert_eq!((c.q16_us, c.area16_us), (900, 3900));
    }

    #[test]
    fn test_reject_a_log_without_a_gate_scene() {
        assert!(read_log("x", "gpud: gpu work (ns_per_px=13)\n").is_none());
    }

    #[test]
    fn the_id_is_the_log_folder_s_cycle() {
        assert_eq!(
            id_of(Path::new("build/logs/board--2026-11-03T19-13-30/uart.log")).as_deref(),
            Some("2026-11-03T19-13-30")
        );
    }

    #[test]
    fn the_constants_carry_the_last_cycle_and_the_table_every_one() {
        let a = read_log("2026-11-02T10-00-00", LOG).expect("a");
        let mut b = a.clone();
        b.id = "2026-11-03T19-13-30".into();
        b.hover_us = 9000;
        let rs = render_constants(&[a.clone(), b.clone()]);
        assert!(rs.contains("cycle: \"2026-11-03T19-13-30\"") && rs.contains("hover_us: 9000"));
        assert!(render_constants(&[]).contains("LAST: Option<Last> = None"));
        let md = render_table(&[a, b]);
        assert_eq!(md.matches("\n| 2026-11-0").count(), 2);
    }
}
