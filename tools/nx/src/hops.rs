// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx hops` — decode a UART log's per-slow-frame `hops one` lines of app-host, windowd
//!          and gpud, join them into one cross-process timeline per frame (windowd `f` is the id,
//!          `from=` names the app-host presents, gpud's `f` is the same seq; records join only when
//!          their t0 lie within 2 s), and print per-class (bare/loaded) hop p50/p90.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/hops.rs
//! ADR: docs/rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md

use crate::error::{ExecResult, ExitClass, NxError};
use clap::Args;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// A seq may wrap: records join only when their t0 lie this close (µs).
const JOIN_WINDOW_US: u64 = 2_000_000;
/// Cross-process stamp order tolerance (µs).
const ORDER_TOLERANCE_US: u64 = 1_000;
/// Console line cap of the UART writers.
const LINE_CAP: usize = 256;
/// Windowd `layers=` bounds of the two classes.
const BARE_MAX_LAYERS: u64 = 8;
const LOADED_MIN_LAYERS: u64 = 16;

/// `nx hops <uart.log>` — cross-process frame timelines from `hops one` lines.
#[derive(Args, Debug)]
pub(crate) struct HopsArgs {
    /// UART log (text; other lines are ignored).
    pub(crate) log: PathBuf,
    /// Only the frame with windowd `f=<N>`.
    #[arg(long)]
    pub(crate) frame: Option<u64>,
    /// Exit non-zero on stamps out of order across processes or lines cut at the console cap.
    #[arg(long)]
    pub(crate) check: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proc {
    App,
    Win,
    Gpu,
}

impl Proc {
    fn name(self) -> &'static str {
        match self {
            Self::App => "apphost",
            Self::Win => "windowd",
            Self::Gpu => "gpud",
        }
    }
}

/// One `hops one` record of one process.
#[derive(Debug, Clone, Default)]
struct Rec {
    t0: u64,
    /// `s=` (app) or `f=` (win/gpu) seq.
    seq: u64,
    /// app: its own `<surface>.<seq>`; win: the `from=` list.
    ids: Vec<(u64, u64)>,
    hops: Vec<(String, u64)>,
    waits: (u64, u64),
    yields: u64,
    cp: u64,
    layers: u64,
    shown: Option<u64>,
    /// Remaining fields printed verbatim (rows, px, rects, glass …).
    extra: Vec<(String, String)>,
    used: bool,
}

impl Rec {
    fn sum(&self) -> u64 {
        self.hops.iter().map(|(_, v)| *v).sum()
    }

    /// The hops up to (and including) the one ending at stamp `last`.
    fn sum_to(&self, last: &str) -> u64 {
        let mut s = 0;
        for (name, v) in &self.hops {
            s += *v;
            if name.ends_with(&format!(">{last}")) {
                break;
            }
        }
        s
    }

    /// Absolute time of the stamp named `name` (the first stamp is t0).
    fn stamp(&self, name: &str) -> Option<u64> {
        let mut t = self.t0;
        let (first, _) = self.hops.first()?.0.split_once('>')?;
        if first == name {
            return Some(t);
        }
        for (hop, v) in &self.hops {
            t = t.saturating_add(*v);
            if hop.split_once('>').map(|(_, b)| b) == Some(name) {
                return Some(t);
            }
        }
        None
    }
}

#[derive(Default)]
struct Parsed {
    apps: Vec<Rec>,
    wins: Vec<Rec>,
    gpus: Vec<Rec>,
    cut: Vec<String>,
    malformed: usize,
}

fn parse_id(v: &str) -> Option<(u64, u64)> {
    let (a, b) = v.split_once('.')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn parse_rec(proc: Proc, body: &str) -> Option<Rec> {
    let mut r = Rec::default();
    let mut have_t0 = false;
    let mut have_seq = false;
    for kv in body.split_whitespace() {
        let (k, v) = kv.split_once('=')?;
        match (proc, k) {
            (Proc::App, "s") => {
                r.ids = vec![parse_id(v)?];
                r.seq = r.ids[0].1;
                have_seq = true;
            }
            (Proc::Win | Proc::Gpu, "f") => {
                r.seq = v.parse().ok()?;
                have_seq = true;
            }
            (_, "t0") => {
                r.t0 = v.parse().ok()?;
                have_t0 = true;
            }
            (Proc::Win, "from") => {
                let list = v.split('+').next().unwrap_or("");
                for id in list.split(',').filter(|s| !s.is_empty()) {
                    r.ids.push(parse_id(id)?);
                }
                if let Some(more) = v.split_once('+') {
                    r.extra.push(("more".into(), more.1.into()));
                }
            }
            (_, "shown") => r.shown = Some(v.parse().ok()?),
            (_, "w") => {
                let (n, us) = v.split_once('/')?;
                r.waits = (n.parse().ok()?, us.parse().ok()?);
            }
            (_, "y") => r.yields = v.parse().ok()?,
            (_, "cp") => r.cp = v.parse().ok()?,
            (Proc::Win, "layers") => {
                r.layers = v.parse().ok()?;
                r.extra.push((k.into(), v.into()));
            }
            _ if k.contains('>') => r.hops.push((k.to_string(), v.parse().ok()?)),
            _ => r.extra.push((k.to_string(), v.to_string())),
        }
    }
    (have_t0 && have_seq && !r.hops.is_empty()).then_some(r)
}

fn parse_log(text: &str) -> Parsed {
    let mut p = Parsed::default();
    for line in text.lines() {
        let tags = [
            ("apphost: hops one (", Proc::App),
            ("windowd: hops one (", Proc::Win),
            ("gpud: hops one (", Proc::Gpu),
        ];
        let Some((tag, proc, at)) =
            tags.iter().find_map(|(t, pr)| line.find(t).map(|i| (*t, *pr, i)))
        else {
            continue;
        };
        let rest = &line[at + tag.len()..];
        let Some(end) = rest.rfind(')') else {
            // No closing paren: the console cut the line. Never parsed as complete.
            p.cut.push(format!("{} (line of {} bytes, cap {LINE_CAP})", proc.name(), line.len()));
            continue;
        };
        match parse_rec(proc, &rest[..end]) {
            Some(r) => match proc {
                Proc::App => p.apps.push(r),
                Proc::Win => p.wins.push(r),
                Proc::Gpu => p.gpus.push(r),
            },
            None => p.malformed += 1,
        }
    }
    p
}

fn near(a: u64, b: u64) -> u64 {
    a.abs_diff(b)
}

/// The nearest unused record of `pool` with the same seq whose t0 is within the join window.
fn take_nearest(pool: &mut [Rec], pred: impl Fn(&Rec) -> bool, t0: u64) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (i, r) in pool.iter().enumerate() {
        if r.used || !pred(r) {
            continue;
        }
        let d = near(r.t0, t0);
        if d <= JOIN_WINDOW_US && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    let (i, _) = best?;
    pool[i].used = true;
    Some(i)
}

struct Frame {
    win: Rec,
    gpu: Option<Rec>,
    apps: Vec<Rec>,
    /// from-ids of the windowd record that no app-host record was printed for.
    apps_missing: usize,
}

struct Joined {
    frames: Vec<Frame>,
    lone_apps: usize,
    lone_gpus: usize,
    wins_without_gpu: usize,
}

fn join(mut p: Parsed) -> Joined {
    p.wins.sort_by_key(|r| r.t0);
    let mut frames = Vec::new();
    let mut wins_without_gpu = 0;
    for win in std::mem::take(&mut p.wins) {
        let seq = win.seq;
        let gpu = take_nearest(&mut p.gpus, |r| r.seq == seq, win.t0).map(|i| p.gpus[i].clone());
        if gpu.is_none() {
            wins_without_gpu += 1;
        }
        let mut apps = Vec::new();
        let mut missing = 0;
        for id in &win.ids {
            match take_nearest(&mut p.apps, |r| r.ids.first() == Some(id), win.t0) {
                Some(i) => apps.push(p.apps[i].clone()),
                None => missing += 1,
            }
        }
        frames.push(Frame { win, gpu, apps, apps_missing: missing });
    }
    Joined {
        frames,
        lone_apps: p.apps.iter().filter(|r| !r.used).count(),
        lone_gpus: p.gpus.iter().filter(|r| !r.used).count(),
        wins_without_gpu,
    }
}

fn class_of(layers: u64) -> &'static str {
    if layers <= BARE_MAX_LAYERS {
        "bare"
    } else if layers >= LOADED_MIN_LAYERS {
        "loaded"
    } else {
        "mid"
    }
}

fn pct(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

fn fmt_extra(r: &Rec) -> String {
    r.extra.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
}

fn render_frame(f: &Frame, out: &mut String) {
    let mut t_min = f.win.t0;
    for a in &f.apps {
        t_min = t_min.min(a.t0);
    }
    if let Some(g) = &f.gpu {
        t_min = t_min.min(g.t0);
    }
    let _ = writeln!(
        out,
        "frame f={} class={} layers={}",
        f.win.seq,
        class_of(f.win.layers),
        f.win.layers
    );
    let mut rows: Vec<(Proc, &Rec, Option<String>)> = Vec::new();
    for a in &f.apps {
        let tag = a.ids.first().map(|(s, q)| format!("s={s}.{q}"));
        rows.push((Proc::App, a, tag));
    }
    rows.push((Proc::Win, &f.win, Some(format!("f={}", f.win.seq))));
    if let Some(g) = &f.gpu {
        rows.push((Proc::Gpu, g, Some(format!("f={}", g.seq))));
    }
    let mut best: Option<(u64, String)> = None;
    for (proc, r, tag) in &rows {
        let _ = write!(
            out,
            "  {:<8} {} +{}us:",
            proc.name(),
            tag.clone().unwrap_or_default(),
            r.t0 - t_min
        );
        for (h, v) in &r.hops {
            let _ = write!(out, " {h}={v}");
            if best.as_ref().is_none_or(|(b, _)| v > b) {
                best = Some((*v, format!("{}.{h}", proc.name())));
            }
        }
        let _ = write!(out, " | w={}/{} y={} cp={}", r.waits.0, r.waits.1, r.yields, r.cp);
        if let Some(s) = r.shown {
            let _ = write!(out, " shown=+{}us", s.saturating_sub(t_min));
        }
        let e = fmt_extra(r);
        if !e.is_empty() {
            let _ = write!(out, " {e}");
        }
        out.push('\n');
    }
    if f.apps.is_empty() {
        let _ = writeln!(out, "  apphost: not slow");
    } else if f.apps_missing > 0 {
        let _ = writeln!(out, "  apphost: {} from-id(s) without a record", f.apps_missing);
    }
    if f.gpu.is_none() {
        let _ = writeln!(out, "  gpud: no record");
    }
    let end = f
        .gpu
        .as_ref()
        .map(|g| g.t0 + g.sum())
        .into_iter()
        .chain(f.win.shown)
        .chain(Some(f.win.t0 + f.win.sum()))
        .max()
        .unwrap_or(t_min);
    let total = end.saturating_sub(t_min);
    if let Some((v, name)) = best {
        let share = if total > 0 { v * 100 / total } else { 0 };
        let _ = writeln!(out, "  total={total}us longest hop: {name}={v}us ({share}% of frame)");
    }
    for (proc, gap) in gaps(f) {
        let _ = writeln!(out, "  gap {proc}={gap}us");
    }
}

/// gap = a process's span minus the sum of its hops — the waits (`w=`) are inside the hops, so
/// they are not subtracted again (a first cut did, and gaps went negative). What is left is time
/// no hop names: the process runnable and not run, a stamp missing, or the IPC between two
/// processes. Span: app t0 to windowd's rx; windowd rx to gpud's rx against its hops up to `sent`
/// (its later `sent>acked` waits on gpud) — without a gpud record there is no outside end to
/// measure it against, and no windowd gap is named; gpud rx to windowd's `shown` (else its last
/// stamp).
fn gaps(f: &Frame) -> Vec<(&'static str, i64)> {
    let mut v = Vec::new();
    let sub = |span: u64, r: &Rec| span as i64 - r.sum() as i64;
    for a in &f.apps {
        v.push(("apphost", sub(f.win.t0.saturating_sub(a.t0), a)));
    }
    if let Some(g) = &f.gpu {
        v.push(("windowd", g.t0 as i64 - f.win.t0 as i64 - f.win.sum_to("sent") as i64));
    }
    if let Some(g) = &f.gpu {
        let end = f.win.shown.unwrap_or(g.t0 + g.sum());
        v.push(("gpud", sub(end.saturating_sub(g.t0), g)));
    }
    v
}

fn check(j: &Joined, cut: &[String], frames: &[&Frame]) -> Vec<String> {
    let mut bad = Vec::new();
    for c in cut {
        bad.push(format!("line cut at the console cap: {c}"));
    }
    let _ = j;
    for f in frames {
        for a in &f.apps {
            if let Some(sent) = a.stamp("sent") {
                if sent > f.win.t0 + ORDER_TOLERANCE_US {
                    bad.push(format!(
                        "f={}: apphost sent ({sent}) after windowd rx ({}) by more than 1 ms",
                        f.win.seq, f.win.t0
                    ));
                }
            }
        }
        if let (Some(g), Some(sent)) = (&f.gpu, f.win.stamp("sent")) {
            if sent > g.t0 + ORDER_TOLERANCE_US {
                bad.push(format!(
                    "f={}: windowd sent ({sent}) after gpud rx ({}) by more than 1 ms",
                    f.win.seq, g.t0
                ));
            }
        }
    }
    bad
}

pub(crate) fn handle_hops(args: HopsArgs) -> ExecResult {
    let bytes = std::fs::read(&args.log).map_err(|e| {
        NxError::new(ExitClass::Usage, format!("cannot read {}: {e}", args.log.display()))
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let parsed = parse_log(&text);
    let cut = parsed.cut.clone();
    let malformed = parsed.malformed;
    let joined = join(parsed);
    let shown: Vec<&Frame> =
        joined.frames.iter().filter(|f| args.frame.is_none_or(|n| f.win.seq == n)).collect();
    let mut out = String::new();
    for f in &shown {
        render_frame(f, &mut out);
    }
    // Per class, per hop percentiles.
    type Series = BTreeMap<String, Vec<u64>>;
    let mut classes: BTreeMap<&str, (usize, Series)> = BTreeMap::new();
    for f in &shown {
        let (n, series) = classes.entry(class_of(f.win.layers)).or_default();
        *n += 1;
        let recs = f
            .apps
            .iter()
            .map(|r| (Proc::App, r))
            .chain([(Proc::Win, &f.win)])
            .chain(f.gpu.as_ref().map(|g| (Proc::Gpu, g)));
        for (proc, r) in recs {
            for (h, v) in &r.hops {
                series.entry(format!("{}.{h}", proc.name())).or_default().push(*v);
            }
        }
    }
    for class in ["bare", "mid", "loaded"] {
        let Some((n, series)) = classes.get_mut(class) else { continue };
        let _ = writeln!(out, "class {class}: {n} frame(s)");
        for (hop, vals) in series.iter_mut() {
            vals.sort_unstable();
            let _ = writeln!(
                out,
                "  {hop:<24} p50={}us p90={}us n={}",
                pct(vals, 50),
                pct(vals, 90),
                vals.len()
            );
        }
    }
    let _ = writeln!(
        out,
        "unjoined: windowd without gpud={} gpud without windowd={} apphost without windowd={} \
         frames without apphost={} cut lines={} malformed lines={}",
        joined.wins_without_gpu,
        joined.lone_gpus,
        joined.lone_apps,
        shown.iter().filter(|f| f.apps.is_empty()).count(),
        cut.len(),
        malformed
    );
    for c in &cut {
        let _ = writeln!(out, "cut: {c}");
    }
    let out = out.trim_end().to_string();
    if args.check {
        let bad = check(&joined, &cut, &shown);
        if !bad.is_empty() {
            let mut msg = out;
            msg.push_str("\ncheck FAILED:");
            for b in bad {
                msg.push_str("\n  ");
                msg.push_str(&b);
            }
            return Err(NxError::new(ExitClass::ValidationReject, msg));
        }
        return Ok((ExitClass::Success, format!("{out}\ncheck ok"), false, None));
    }
    Ok((ExitClass::Success, out, false, None))
}
