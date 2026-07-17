// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0068 structured-event observability — the shared, alloc-free core.
//! OWNERS: @runtime
//! STATUS: Functional (P1: types + verdict math)
//! API_STABILITY: Unstable (RFC-0068 evolves the model between phases)
//!
//! This crate is the single source of truth for the OBSERVABILITY VERDICT MATH and the structured
//! event vocabulary. Both the kernel (`diag::log`) and userspace (`nexus-log` / `nexus-abi`) build
//! on it, so the per-group `N/N OK <ms>` aggregation is defined and TESTED in one place instead of
//! being copied per side (it currently lives twice: `nexus_abi::SVC_*` and the kernel `diag` GROUP
//! table — both fold into this).
//!
//! Design (RFC-0068): emit structured EVENTS in SPANS, scoped by SUBJECT (the subsystem a record is
//! ABOUT, not who emitted it). Alloc-free by construction: fixed records, plain integer aggregation,
//! no heap / no `Vec` / no `format!` — the kernel UART + allocator constraint that shaped the
//! existing solution holds here.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// Severity. Mirrors `nexus_log::Level` and the kernel `diag::log::Level` so the three share one
/// policy vocabulary.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

/// The PROOF status of an event — the structural anti-"fake-proof" field (RFC-0068 §6). `Ok` is set
/// only on a real, verified check; `Lifecycle` is "a state was reached" (e.g. `service: ready`) and
/// is explicitly NOT a proof. A renderer/verifier inspects this field instead of pattern-matching
/// the text `"ok"`, so a hollow lifecycle marker can never masquerade as a passing proof.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// A real check passed.
    Ok,
    /// Completed but degraded / over budget — counts as passing but is surfaced.
    Warn,
    /// A check failed — always counts against the verdict and always prints live.
    Fail,
    /// A state was reached (lifecycle), not a proof of correctness.
    Lifecycle,
}

impl Status {
    #[must_use]
    pub fn is_fail(self) -> bool {
        matches!(self, Status::Fail)
    }
}

/// The subsystem an event is ABOUT — first-class and independent of the emitter (RFC-0068 §3).
/// Equality groups records across process boundaries: `init`'s capability grant for policyd and
/// policyd's own boot markers share `Subject("policyd")` even though different processes emit them.
/// A short stable name: services use their package name; the kernel its subsystem names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Subject(pub &'static str);

impl Subject {
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.0
    }

    // Well-known kernel/init subjects (services use their package name).
    pub const BOOT: Subject = Subject("boot");
    pub const AS: Subject = Subject("as");
    pub const SMP: Subject = Subject("smp");
    pub const SYSCALL: Subject = Subject("syscall");
    pub const SYS: Subject = Subject("sys");
    pub const KSELF: Subject = Subject("kself");
    pub const INIT: Subject = Subject("init");
}

/// A structured event: the future replacement for a raw text line (RFC-0068 §1). Bounded inline
/// `name` (no heap); structured `fields` land in P4's wire record. P1 carries the routing core.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub ts_ns: u64,
    pub level: Level,
    pub subject: Subject,
    pub name: &'static str,
    pub status: Status,
}

/// Soft-real-time budget (ms): a span that PASSED but ran at least this long renders `WARN … slow`,
/// so a sluggish subsystem (the "a service took 12 s" case) stands out of an otherwise quiet `OK`
/// column.
pub const SLOW_BUDGET_MS: u64 = 250;

/// The rendered verdict tag for a span.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerdictTag {
    Ok,
    Warn,
    Error,
}

impl VerdictTag {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            VerdictTag::Ok => "OK",
            VerdictTag::Warn => "WARN",
            VerdictTag::Error => "ERROR",
        }
    }

    /// True when the span passed but was slow — the renderer appends a `slow` suffix.
    #[must_use]
    pub fn is_slow(self) -> bool {
        matches!(self, VerdictTag::Warn)
    }
}

/// One subject-span's aggregated verdict. The grid line `[ts] TAG subject passed/total <ms>[ slow]`
/// renders from this.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub passed: u32,
    pub total: u32,
    pub ms: u64,
    pub tag: VerdictTag,
}

/// The verdict math in ONE place, for callers that hold their own counters (the `nexus-abi`
/// per-process wrapper and the kernel `diag` GROUP table keep separate atomics and feed this rather
/// than materializing a [`SpanTally`]). `started_at` is the span's start time, `None` if the span
/// recorded nothing. Measuring from an explicit `Option` (not a `0`-sentinel) keeps it correct even
/// when a monotonic clock legitimately reads `0` at the very first marker.
#[must_use]
pub fn verdict_from(total: u32, fails: u32, started_at: Option<u64>, flush_ns: u64) -> Verdict {
    let passed = total.saturating_sub(fails);
    let ms = match started_at {
        Some(start) if flush_ns >= start => (flush_ns - start) / 1_000_000,
        _ => 0,
    };
    let tag = if fails != 0 {
        VerdictTag::Error
    } else if ms >= SLOW_BUDGET_MS {
        VerdictTag::Warn
    } else {
        VerdictTag::Ok
    };
    Verdict { passed, total, ms, tag }
}

/// Alloc-free per-span aggregator — the SSOT for the verdict math. Plain fields; the caller owns
/// any synchronization (the per-process wrapper in `nexus-abi` and the kernel `diag` GROUP table
/// hold atomics and feed [`verdict_from`] directly). No heap, no replay buffer: failures print live
/// at the emit site, this only counts.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpanTally {
    total: u32,
    fails: u32,
    first_ns: u64,
    last_ns: u64,
    started: bool,
}

impl SpanTally {
    #[must_use]
    pub const fn new() -> Self {
        Self { total: 0, fails: 0, first_ns: 0, last_ns: 0, started: false }
    }

    /// Explicitly stamp the span start (e.g. at bootstrap-arm, before the first event) so the
    /// duration covers setup→first-event too. Idempotent — only the first stamp wins, so a later
    /// [`record`](Self::record) does not move it.
    pub fn start(&mut self, now_ns: u64) {
        if !self.started {
            self.first_ns = now_ns;
            self.last_ns = now_ns;
            self.started = true;
        }
    }

    /// Record one event at `now_ns`. Stamps the start if not already started, and advances the
    /// span END to `now_ns`. Returns `true` if it was a failure (the caller prints failures live and
    /// never suppresses them).
    pub fn record(&mut self, status: Status, now_ns: u64) -> bool {
        self.start(now_ns);
        if now_ns > self.last_ns {
            self.last_ns = now_ns;
        }
        self.total = self.total.saturating_add(1);
        let failed = status.is_fail();
        if failed {
            self.fails = self.fails.saturating_add(1);
        }
        failed
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// The span's start time, or `None` if it never started — the timing anchor.
    #[must_use]
    pub fn started_at(&self) -> Option<u64> {
        if self.started {
            Some(self.first_ns)
        } else {
            None
        }
    }

    /// Compute the verdict, measuring duration from the span start to `flush_ns` (the close time).
    /// `WARN` (slow) when it passed but ran ≥ [`SLOW_BUDGET_MS`]; `ERROR` on any failure.
    #[must_use]
    pub fn verdict(&self, flush_ns: u64) -> Verdict {
        verdict_from(self.total, self.fails, self.started_at(), flush_ns)
    }

    /// Compute the verdict over the SELF-CONTAINED span — first event to LAST event — independent of
    /// when the caller flushes. Use this when the flush happens far after the last event (e.g. a
    /// collector drained at end-of-phase): the duration then reflects the work itself, not the wait
    /// until the drain. For an instant burst of events this is ~0 ms.
    #[must_use]
    pub fn verdict_self(&self) -> Verdict {
        let end = if self.started { Some(self.last_ns) } else { None };
        verdict_from(self.total, self.fails, self.started_at(), end.unwrap_or(0))
    }
}

/// A bounded, alloc-free SUBJECT collector (RFC-0068 §4) — the mechanism that makes CROSS-PROCESS
/// subject grouping possible. A single per-process counter can only fold its own lines; to fold
/// records about the same subject emitted by DIFFERENT processes (init's `policyd` grant + policyd's
/// own boot markers) you need ONE point that maps `subject → tally`. That point is a central journal
/// (logd) or any process that handles several subjects (init wiring N services). Capacity `N` is
/// fixed; once full, an unseen subject's record is counted nowhere but still reported as
/// pass/fail so the caller can print it live (no record is silently lost). No heap.
pub struct VerdictTable<const N: usize> {
    subjects: [Option<Subject>; N],
    tallies: [SpanTally; N],
    len: usize,
}

impl<const N: usize> Default for VerdictTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> VerdictTable<N> {
    #[must_use]
    pub const fn new() -> Self {
        Self { subjects: [None; N], tallies: [SpanTally::new(); N], len: 0 }
    }

    /// Find the slot index for `subject`, inserting a fresh tally if new and capacity remains.
    fn slot(&mut self, subject: Subject) -> Option<usize> {
        for i in 0..self.len {
            if self.subjects[i] == Some(subject) {
                return Some(i);
            }
        }
        if self.len < N {
            self.subjects[self.len] = Some(subject);
            self.len += 1;
            Some(self.len - 1)
        } else {
            None
        }
    }

    /// Record one event for `subject` at `now_ns`. Records from different emitters that name the
    /// same subject merge into that subject's one tally — the cross-process fold. Returns `true` on
    /// failure (the caller prints failures live and never suppresses them, even when the table is
    /// full and the event could not be counted).
    pub fn record(&mut self, subject: Subject, status: Status, now_ns: u64) -> bool {
        match self.slot(subject) {
            Some(i) => self.tallies[i].record(status, now_ns),
            None => status.is_fail(),
        }
    }

    /// Explicitly stamp a subject's span start (e.g. when init begins wiring a service), so its
    /// duration covers setup→first-event. Idempotent per subject; a no-op if the table is full.
    pub fn start(&mut self, subject: Subject, now_ns: u64) {
        if let Some(i) = self.slot(subject) {
            self.tallies[i].start(now_ns);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Visit `(subject, verdict)` for each recorded subject in insertion order, each verdict measured
    /// against `flush_ns`. The renderer calls [`render_verdict_line`] per entry to emit the grid.
    pub fn for_each_verdict(&self, flush_ns: u64, mut f: impl FnMut(Subject, Verdict)) {
        for i in 0..self.len {
            if let Some(s) = self.subjects[i] {
                f(s, self.tallies[i].verdict(flush_ns));
            }
        }
    }

    /// Like [`for_each_verdict`](Self::for_each_verdict) but each subject's verdict spans first→last
    /// of ITS OWN events ([`SpanTally::verdict_self`]) — for a collector drained long after the work,
    /// so each row's duration reflects that subject's work, not the wait until the drain.
    pub fn for_each_verdict_self(&self, mut f: impl FnMut(Subject, Verdict)) {
        for i in 0..self.len {
            if let Some(s) = self.subjects[i] {
                f(s, self.tallies[i].verdict_self());
            }
        }
    }
}

/// A `core::fmt::Write` adapter over a caller-owned byte slice — the alloc-free backing for
/// [`render_verdict_line`]. Writes past the end are silently dropped (the line is bounded).
struct SliceWriter<'a> {
    buf: &'a mut [u8],
    n: usize,
}

impl core::fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &c in s.as_bytes() {
            if self.n < self.buf.len() {
                self.buf[self.n] = c;
                self.n += 1;
            }
        }
        Ok(())
    }
}

/// Render ONE verdict grid line into `buf`, returning the number of bytes written (clamped to the
/// buffer). This is the SSOT for the console grid format (RFC-0068 §5 — rendering is one concern):
/// `[    S.uuuuuu]  TAG    subject        passed/total   Nms[  slow]\n`. Both the per-process
/// flush (`nexus-abi`) and the kernel GROUP flush call this so the column layout can never drift
/// between the two. Alloc-free: the caller owns the buffer (96 bytes covers the longest line) and
/// writes the rendered slice in one atomic console write.
#[must_use]
pub fn render_verdict_line(buf: &mut [u8], now_ns: u64, subject: &str, v: Verdict) -> usize {
    use core::fmt::Write as _;
    let mut w = SliceWriter { buf, n: 0 };
    let _ = writeln!(
        w,
        "[{:>5}.{:06}]  {:<6} {:<14} {}/{}   {}ms{}",
        now_ns / 1_000_000_000,
        (now_ns % 1_000_000_000) / 1000,
        v.tag.label(),
        subject,
        v.passed,
        v.total,
        v.ms,
        if v.tag.is_slow() { "  slow" } else { "" },
    );
    w.n
}

/// Render the grid TIMESTAMP PREFIX `[    S.uuuuuu]  ` into `buf` (RFC-0068 — the same leading column
/// as [`render_verdict_line`]), returning bytes written. For emitters that build a marker line from
/// fragments and want to prepend the timestamp themselves.
#[must_use]
pub fn render_ts_prefix(buf: &mut [u8], now_ns: u64) -> usize {
    use core::fmt::Write as _;
    let mut w = SliceWriter { buf, n: 0 };
    let _ = write!(w, "[{:>5}.{:06}]  ", now_ns / 1_000_000_000, (now_ns % 1_000_000_000) / 1000);
    w.n
}

/// Render ONE timestamped marker line `[    S.uuuuuu]  {body}\n` into `buf` (RFC-0068). The unfolded
/// analogue of [`render_verdict_line`]: gives a raw bring-up/runtime marker the same `[ts]` column as
/// the verdict grid so the boot timeline is legible in `build/logs`. Returns bytes written (clamped).
#[must_use]
pub fn render_marker_ts(buf: &mut [u8], now_ns: u64, body: &str) -> usize {
    use core::fmt::Write as _;
    let mut w = SliceWriter { buf, n: 0 };
    let _ = writeln!(
        w,
        "[{:>5}.{:06}]  {}",
        now_ns / 1_000_000_000,
        (now_ns % 1_000_000_000) / 1000,
        body,
    );
    w.n
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn all_ok_is_ok() {
        let mut t = SpanTally::new();
        let start = 1_000 * MS;
        for i in 0..53 {
            t.record(Status::Ok, start + i * MS);
        }
        let v = t.verdict(start + 14 * MS); // 14 ms span
        assert_eq!(v, Verdict { passed: 53, total: 53, ms: 14, tag: VerdictTag::Ok });
        assert_eq!(v.tag.label(), "OK");
        assert!(!v.tag.is_slow());
    }

    #[test]
    fn any_fail_is_error_and_counts() {
        let mut t = SpanTally::new();
        let s = 0;
        assert!(!t.record(Status::Ok, s));
        assert!(!t.record(Status::Lifecycle, s + MS)); // lifecycle is not a failure
        assert!(t.record(Status::Fail, s + 2 * MS)); // returns true on failure
        let v = t.verdict(s + 3 * MS);
        assert_eq!(v.passed, 2);
        assert_eq!(v.total, 3);
        assert_eq!(v.tag, VerdictTag::Error);
    }

    #[test]
    fn slow_pass_is_warn_slow() {
        let mut t = SpanTally::new();
        let s = 0;
        t.record(Status::Ok, s);
        let v = t.verdict(s + SLOW_BUDGET_MS * MS); // exactly the budget → slow
        assert_eq!(v.tag, VerdictTag::Warn);
        assert!(v.tag.is_slow());
        assert_eq!(v.ms, SLOW_BUDGET_MS);

        // just under budget → OK
        let v2 = t.verdict(s + (SLOW_BUDGET_MS - 1) * MS);
        assert_eq!(v2.tag, VerdictTag::Ok);
    }

    #[test]
    fn empty_span_has_no_verdict_worth_emitting() {
        let t = SpanTally::new();
        assert!(t.is_empty());
        let v = t.verdict(999 * MS);
        assert_eq!(v.total, 0);
    }

    #[test]
    fn fail_dominates_even_if_slow() {
        let mut t = SpanTally::new();
        t.record(Status::Fail, 0);
        // slow AND failed → ERROR wins over WARN
        assert_eq!(t.verdict(SLOW_BUDGET_MS * MS).tag, VerdictTag::Error);
    }

    #[test]
    fn start_at_ts_zero_is_not_unset() {
        // A monotonic clock may legitimately read 0 at the first marker; that must NOT be confused
        // with "never started" (the bug the Option<u64> anchor fixes).
        let mut t = SpanTally::new();
        assert_eq!(t.started_at(), None);
        t.record(Status::Ok, 0);
        assert_eq!(t.started_at(), Some(0));
        // 14 ms later still measures correctly from 0.
        assert_eq!(t.verdict(14 * MS).ms, 14);
    }

    #[test]
    fn explicit_start_covers_setup_before_first_event() {
        // arm-at-bootstrap: start() stamps before any event so duration includes setup→first.
        let mut t = SpanTally::new();
        t.start(100 * MS);
        t.record(Status::Ok, 105 * MS); // record must NOT move the start
        assert_eq!(t.started_at(), Some(100 * MS));
        assert_eq!(t.verdict(110 * MS).ms, 10);
    }

    #[test]
    fn verdict_from_matches_tally() {
        // The free function (for atomic callers) agrees with the struct path.
        let v = verdict_from(5, 1, Some(0), 12 * MS);
        assert_eq!(v, Verdict { passed: 4, total: 5, ms: 12, tag: VerdictTag::Error });
        // None anchor → no duration even with a large flush time.
        assert_eq!(verdict_from(2, 0, None, 999 * MS).ms, 0);
    }

    fn canonical(now: u64, subject: &str, v: Verdict) -> String {
        format!(
            "[{:>5}.{:06}]  {:<6} {:<14} {}/{}   {}ms{}\n",
            now / 1_000_000_000,
            (now % 1_000_000_000) / 1000,
            v.tag.label(),
            subject,
            v.passed,
            v.total,
            v.ms,
            if v.tag.is_slow() { "  slow" } else { "" },
        )
    }

    #[test]
    fn render_matches_canonical_grid_format() {
        // Pins the byte layout both the kernel flush_group and nexus-abi flush previously inlined —
        // render_verdict_line must reproduce it exactly so swapping the call sites is byte-stable.
        let cases = [
            (
                163_015_000u64,
                "kself",
                Verdict { passed: 53, total: 53, ms: 14, tag: VerdictTag::Ok },
            ),
            (
                1_716_000_000,
                "windowd",
                Verdict { passed: 22, total: 22, ms: 1716, tag: VerdictTag::Warn },
            ),
            (
                360_000_000,
                "selftest",
                Verdict { passed: 25, total: 27, ms: 360, tag: VerdictTag::Error },
            ),
        ];
        for (now, subj, v) in cases {
            let mut buf = [0u8; 96];
            let n = render_verdict_line(&mut buf, now, subj, v);
            assert_eq!(&buf[..n], canonical(now, subj, v).as_bytes(), "subject={subj}");
        }
    }

    #[test]
    fn render_slow_suffix_only_on_warn() {
        let mk = |tag| {
            let mut b = [0u8; 96];
            let n =
                render_verdict_line(&mut b, 0, "x", Verdict { passed: 1, total: 1, ms: 0, tag });
            core::str::from_utf8(&b[..n]).unwrap().to_string()
        };
        assert!(mk(VerdictTag::Warn).contains("slow"));
        assert!(!mk(VerdictTag::Ok).contains("slow"));
        assert!(!mk(VerdictTag::Error).contains("slow"));
    }

    #[test]
    fn render_truncates_into_small_buffer_without_panic() {
        let mut buf = [0u8; 8];
        let n = render_verdict_line(
            &mut buf,
            0,
            "verylongsubjectname",
            Verdict { passed: 1, total: 1, ms: 0, tag: VerdictTag::Ok },
        );
        assert!(n <= 8);
    }

    #[test]
    fn verdict_self_measures_first_to_last_event_not_flush() {
        // The fix for the inflated init:<svc> ms: a span is bounded by its EVENTS, not by when the
        // collector is drained. flush far away → big ms; verdict_self → first→last only.
        let mut t = SpanTally::new();
        t.record(Status::Ok, 100 * MS);
        t.record(Status::Ok, 105 * MS);
        assert_eq!(t.verdict(900 * MS).ms, 800); // drain-relative (old behaviour)
        assert_eq!(t.verdict_self().ms, 5); // self-contained span
        assert_eq!(t.verdict_self().tag, VerdictTag::Ok);
    }

    #[test]
    fn table_verdict_self_is_per_subject_span() {
        let mut t = VerdictTable::<4>::new();
        t.record(Subject("a"), Status::Ok, 10 * MS);
        t.record(Subject("a"), Status::Ok, 12 * MS); // a spans 2 ms
        t.record(Subject("b"), Status::Ok, 50 * MS); // b is a single instant event
        let mut got = std::collections::BTreeMap::new();
        t.for_each_verdict_self(|s, v| {
            got.insert(s.name(), v.ms);
        });
        assert_eq!(got["a"], 2);
        assert_eq!(got["b"], 0);
    }

    #[test]
    fn subject_groups_by_name_across_emitters() {
        // The whole point of RFC-0068: same subject, different emitters, one group.
        let from_init = Subject("policyd");
        let from_service = Subject("policyd");
        assert_eq!(from_init, from_service);
        assert_ne!(Subject("policyd"), Subject::INIT);
        assert_eq!(Subject::KSELF.name(), "kself");
    }

    #[test]
    fn verdict_table_folds_one_subject_across_two_emitters() {
        // THE P4 proof: records about `policyd` emitted by the init "process" and the policyd
        // "process" fold into ONE verdict — the cross-process grouping a per-process counter cannot do.
        let mut t = VerdictTable::<8>::new();
        // init wiring policyd (3 grant/wire steps)
        t.record(Subject("policyd"), Status::Ok, 10 * MS);
        t.record(Subject("policyd"), Status::Ok, 11 * MS);
        t.record(Subject("policyd"), Status::Lifecycle, 12 * MS);
        // policyd's own boot markers (different emitter, same subject)
        t.record(Subject("policyd"), Status::Ok, 40 * MS);
        t.record(Subject("policyd"), Status::Ok, 41 * MS);
        assert_eq!(t.len(), 1, "one subject, not two emitter-groups");
        let mut seen = None;
        t.for_each_verdict(45 * MS, |s, v| {
            assert_eq!(s, Subject("policyd"));
            seen = Some(v);
        });
        let v = seen.unwrap();
        assert_eq!(v.total, 5); // all 5 records from both emitters
        assert_eq!(v.passed, 5);
        assert_eq!(v.ms, 35); // 10ms (first) → 45ms (flush)
        assert_eq!(v.tag, VerdictTag::Ok);
    }

    #[test]
    fn verdict_table_keeps_distinct_subjects_separate() {
        let mut t = VerdictTable::<8>::new();
        t.record(Subject("gpud"), Status::Ok, 0);
        t.record(Subject("windowd"), Status::Fail, MS);
        t.record(Subject("gpud"), Status::Ok, 2 * MS);
        assert_eq!(t.len(), 2);
        let mut tags = std::collections::BTreeMap::new();
        t.for_each_verdict(3 * MS, |s, v| {
            tags.insert(s.name(), (v.total, v.tag));
        });
        assert_eq!(tags["gpud"], (2, VerdictTag::Ok));
        assert_eq!(tags["windowd"], (1, VerdictTag::Error));
    }

    #[test]
    fn verdict_table_full_still_reports_failures_live() {
        let mut t = VerdictTable::<2>::new();
        t.record(Subject("a"), Status::Ok, 0);
        t.record(Subject("b"), Status::Ok, 0);
        // table full: a NEW subject can't be counted, but a failure is still reported so the caller
        // prints it live (the user's hard rule: never lose a failure).
        assert!(t.record(Subject("c"), Status::Fail, 0));
        assert!(!t.record(Subject("c"), Status::Ok, 0));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn verdict_table_start_anchors_duration_before_first_event() {
        let mut t = VerdictTable::<4>::new();
        t.start(Subject("svc"), 100 * MS); // init begins wiring
        t.record(Subject("svc"), Status::Ok, 130 * MS); // first real event 30ms later
        let mut ms = None;
        t.for_each_verdict(140 * MS, |_, v| ms = Some(v.ms));
        assert_eq!(ms, Some(40)); // 100 → 140, includes the setup gap
    }
}
