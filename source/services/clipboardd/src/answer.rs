// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one request → one reply. [`Clipboard::answer`] decodes the frame,
//! asks the [`crate::gate`] with the kernel-attributed sender, applies the op
//! to the [`History`] and encodes the reply; the [`Outcome`] names what
//! happened so the loop can write a marker ([`marker`]) without ever touching
//! the contents. A focus push answers nothing (fire-and-forget).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs

use core::fmt::Write as _;

use nexus_wire::clipboardd as wire;

use crate::gate::{self, Access, FocusTruth};
use crate::history::{matches_query, History};

/// What one request did — the marker's input (never carries contents).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// An item was stored. No length: a copied text's length on every copy would
    /// trace what the user typed (the keystroke privacy rule).
    Written { seq: u64 },
    /// An item was read (the paste).
    Read { seq: u64 },
    /// The history was listed (`n` entries matched).
    Listed { n: usize },
    /// An item was made the newest again; `proof` = the first copy-back after
    /// a served list on this boot (the visible chain's witness).
    Restored { seq: u64, proof: bool },
    /// The history was cleared.
    Cleared,
    /// windowd's focus push was applied; `first` = the first one this boot.
    Focus { first: bool },
    /// A focus push from anyone but windowd was dropped.
    FocusRejected,
    /// The gate refused the op.
    Denied { op: &'static str },
    /// Nothing to read or restore (empty history / unknown `seq`).
    Empty { op: &'static str },
    /// The frame did not decode or broke a bound.
    Malformed,
    /// An op this version does not serve.
    Unsupported,
}

/// The authority's whole state.
pub struct Clipboard {
    history: History,
    truth: FocusTruth,
    windowd: u64,
    focus_live: bool,
    history_served: bool,
    proof_said: bool,
}

impl Clipboard {
    /// An empty clipboard that takes focus pushes from `windowd` (its kernel sid).
    #[must_use]
    pub const fn new(windowd: u64) -> Self {
        Self {
            history: History::new(),
            truth: FocusTruth { focused: 0, desktop: 0, ime: 0 },
            windowd,
            focus_live: false,
            history_served: false,
            proof_said: false,
        }
    }

    /// The history (read-only; the tests look at it).
    #[must_use]
    pub fn history(&self) -> &History {
        &self.history
    }

    /// The owners windowd last named.
    #[must_use]
    pub fn truth(&self) -> FocusTruth {
        self.truth
    }

    /// Answers one request from `sender` into `out` (at least
    /// [`wire::REPLY_MAX_BYTES`]). Returns the reply length — 0 = no reply
    /// (a focus push, or an `out` too small) — and what happened.
    pub fn answer(&mut self, frame: &[u8], sender: u64, out: &mut [u8]) -> (usize, Outcome) {
        let Some(op) = wire::decode_request_op(frame) else {
            return (status_reply(0, wire::STATUS_MALFORMED, out), Outcome::Malformed);
        };
        match op {
            wire::OP_FOCUS => (0, self.focus(frame, sender)),
            wire::OP_WRITE => self.write(frame, sender, out),
            wire::OP_READ => self.read(frame, sender, out),
            wire::OP_LIST => self.list(frame, sender, out),
            wire::OP_RESTORE => self.restore(frame, sender, out),
            wire::OP_CLEAR => self.clear(frame, sender, out),
            _ => (status_reply(op, wire::STATUS_UNSUPPORTED, out), Outcome::Unsupported),
        }
    }

    fn focus(&mut self, frame: &[u8], sender: u64) -> Outcome {
        if !gate::is_focus_authority(sender, self.windowd) {
            return Outcome::FocusRejected;
        }
        let Some((focused, desktop, ime)) = wire::decode_focus(frame) else {
            return Outcome::Malformed;
        };
        self.truth = FocusTruth { focused, desktop, ime };
        let first = !self.focus_live;
        self.focus_live = true;
        Outcome::Focus { first }
    }

    fn write(&mut self, frame: &[u8], sender: u64, out: &mut [u8]) -> (usize, Outcome) {
        let op = wire::OP_WRITE;
        let Some(text) = wire::decode_write(frame) else {
            return (status_reply(op, wire::STATUS_MALFORMED, out), Outcome::Malformed);
        };
        if !gate::allows(&self.truth, sender, Access::Write) {
            return (seq_reply(op, wire::STATUS_DENIED, 0, out), Outcome::Denied { op: "write" });
        }
        match self.history.write(text, sender, 0) {
            Some(seq) => (seq_reply(op, wire::STATUS_OK, seq, out), Outcome::Written { seq }),
            None => (seq_reply(op, wire::STATUS_MALFORMED, 0, out), Outcome::Malformed),
        }
    }

    fn read(&mut self, frame: &[u8], sender: u64, out: &mut [u8]) -> (usize, Outcome) {
        let op = wire::OP_READ;
        let Some(seq) = wire::decode_read(frame) else {
            return (status_reply(op, wire::STATUS_MALFORMED, out), Outcome::Malformed);
        };
        if !gate::allows(&self.truth, sender, Access::Read) {
            let n = wire::encode_read_reply(op, wire::STATUS_DENIED, 0, "", out).unwrap_or(0);
            return (n, Outcome::Denied { op: "read" });
        }
        let item = if seq == 0 { self.history.newest() } else { self.history.get(seq) };
        match item {
            Some(item) => {
                let n = wire::encode_read_reply(op, wire::STATUS_OK, item.seq, item.text(), out)
                    .unwrap_or(0);
                (n, Outcome::Read { seq: item.seq })
            }
            None => {
                let n = wire::encode_read_reply(op, wire::STATUS_EMPTY, 0, "", out).unwrap_or(0);
                (n, Outcome::Empty { op: "read" })
            }
        }
    }

    fn list(&mut self, frame: &[u8], sender: u64, out: &mut [u8]) -> (usize, Outcome) {
        let op = wire::OP_LIST;
        let Some(query) = wire::decode_list(frame) else {
            return (status_reply(op, wire::STATUS_MALFORMED, out), Outcome::Malformed);
        };
        if !gate::allows(&self.truth, sender, Access::History) {
            let n = wire::encode_list_reply(op, wire::STATUS_DENIED, 0, &[], out).unwrap_or(0);
            return (n, Outcome::Denied { op: "list" });
        }
        let mut packed = [0u8; wire::LIST_MAX_BYTES];
        let (mut len, mut count) = (0usize, 0u8);
        for item in self.history.iter().filter(|i| matches_query(i.text(), query)) {
            let entry = wire::ListEntry {
                seq: item.seq,
                writer_sid: item.writer_sid,
                device: item.device,
                preview: wire::preview_of(item.text()),
            };
            // Cannot overflow: the history holds at most HISTORY_MAX entries
            // and the buffer is sized for that many full previews.
            let Some(next) = wire::pack_list_entry(&mut packed, len, &entry) else { break };
            len = next;
            count += 1;
        }
        self.history_served = true;
        let n =
            wire::encode_list_reply(op, wire::STATUS_OK, count, &packed[..len], out).unwrap_or(0);
        (n, Outcome::Listed { n: usize::from(count) })
    }

    fn restore(&mut self, frame: &[u8], sender: u64, out: &mut [u8]) -> (usize, Outcome) {
        let op = wire::OP_RESTORE;
        let Some(seq) = wire::decode_restore(frame) else {
            return (status_reply(op, wire::STATUS_MALFORMED, out), Outcome::Malformed);
        };
        if !gate::allows(&self.truth, sender, Access::History) {
            return (seq_reply(op, wire::STATUS_DENIED, 0, out), Outcome::Denied { op: "restore" });
        }
        match self.history.restore(seq) {
            Some(new_seq) => {
                let proof = self.history_served && !self.proof_said;
                self.proof_said |= proof;
                (
                    seq_reply(op, wire::STATUS_OK, new_seq, out),
                    Outcome::Restored { seq: new_seq, proof },
                )
            }
            None => (seq_reply(op, wire::STATUS_EMPTY, 0, out), Outcome::Empty { op: "restore" }),
        }
    }

    fn clear(&mut self, frame: &[u8], sender: u64, out: &mut [u8]) -> (usize, Outcome) {
        let op = wire::OP_CLEAR;
        if wire::decode_clear(frame).is_none() {
            return (status_reply(op, wire::STATUS_MALFORMED, out), Outcome::Malformed);
        }
        if !gate::allows(&self.truth, sender, Access::History) {
            return (status_reply(op, wire::STATUS_DENIED, out), Outcome::Denied { op: "clear" });
        }
        self.history.clear();
        (status_reply(op, wire::STATUS_OK, out), Outcome::Cleared)
    }
}

fn status_reply(op: u8, status: u8, out: &mut [u8]) -> usize {
    let r = wire::encode_status(op, status);
    copy_out(&r, out)
}

fn seq_reply(op: u8, status: u8, seq: u64, out: &mut [u8]) -> usize {
    let r = wire::encode_seq_reply(op, status, seq);
    copy_out(&r, out)
}

fn copy_out(reply: &[u8], out: &mut [u8]) -> usize {
    match out.get_mut(..reply.len()) {
        Some(dst) => {
            dst.copy_from_slice(reply);
            reply.len()
        }
        None => 0,
    }
}

/// A marker line in fixed storage (no heap; a line that does not fit is cut).
pub struct Line {
    buf: [u8; 96],
    len: usize,
}

impl Line {
    /// An empty line.
    #[must_use]
    pub const fn new() -> Self {
        Self { buf: [0; 96], len: 0 }
    }

    /// The text written so far.
    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = self.buf.len() - self.len;
        let take = s.len().min(room);
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

/// The marker for an outcome, if it has one. Never contents and never a
/// content's length — only sequence numbers and counts. Lists have none (one
/// per keystroke while searching would flood the log); denials are the loop's
/// to bound.
pub fn marker(outcome: &Outcome, line: &mut Line) -> bool {
    let r = match *outcome {
        Outcome::Written { seq } => write!(line, "clipboardd: write ok (seq={seq})"),
        Outcome::Restored { seq, .. } => write!(line, "clipboardd: restore ok (seq={seq})"),
        Outcome::Cleared => write!(line, "clipboardd: clear ok"),
        Outcome::Focus { first: true } => write!(line, "clipboardd: focus truth live"),
        Outcome::FocusRejected => write!(line, "clipboardd: focus push REJECT (foreign sender)"),
        Outcome::Denied { op } => write!(line, "clipboardd: {op} deny (reason=clipboard-focus)"),
        Outcome::Malformed => write!(line, "clipboardd: request malformed"),
        Outcome::Unsupported => write!(line, "clipboardd: request unsupported"),
        Outcome::Read { .. }
        | Outcome::Listed { .. }
        | Outcome::Focus { first: false }
        | Outcome::Empty { .. } => return false,
    };
    r.is_ok()
}

/// The visible chain's selftest line (the first copy-back after a served list).
pub const SELFTEST_UI_V7_CLIPBOARD_OK: &str = "SELFTEST: ui v7 clipboard ok";
