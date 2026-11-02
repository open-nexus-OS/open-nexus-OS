// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: inputd's sending end of the key channel (RFC-0113 §1–§3, ADR-0090). Every HID batch's
//! stream sequence is checked — a gap is events hidrawd sent that never arrived (`FAIL`,
//! counted). The keys a batch produced, or the repeat that came due, are translated
//! (`key_channel`: the source decides the route) and sent to windowd in order on the SAME channel
//! as the visible state, so the click that focuses a field always lands before the keys typed
//! after it. Sends BLOCK: a full windowd queue holds inputd, a key is never dropped for it; a
//! dead route is counted and named. While the compositor itself is gone — its restart (RFC-0112):
//! the window a key would go to is not known until its app rejoins the new one — a key is
//! dropped, never held for a window it may not belong to, and the outage named once. The
//! layout's relay to imed (`OP_SET_LAYOUT`) lives here too — the composition engine follows
//! `input.keymap`.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `key_channel` and `seq_stream` on the host; the QEMU typing scenario end to end
//!   (`apphost: input session (`)

use super::*;
use crate::key_channel::{ChannelKey, KeySource, KeyTranslator};
use input_live_protocol::{
    encode_push_key_input, KeyInput, SeqCheck, SeqStreams, KEY_INPUT_FRAME_MAX, MAX_KEY_INPUTS,
};

/// Failure lines a boot prints before the counters speak alone.
const FAIL_LINES_MAX: u32 = 8;

/// The key channel's state: the translation, the streams, the frame being filled.
pub(super) struct KeyChannel {
    translator: KeyTranslator,
    streams: SeqStreams,
    out: Outbox,
    /// The lazily made leg to imed for the layout relay.
    imed: Option<KernelClient>,
}

impl KeyChannel {
    pub(super) const fn new() -> Self {
        Self {
            translator: KeyTranslator::new(),
            streams: SeqStreams::new(),
            out: Outbox {
                frame: [KeyInput {
                    kind: 0,
                    sub: 0,
                    flags: 0,
                    modifiers: 0,
                    seq: 0,
                    t_ns: 0,
                    value: 0,
                }; MAX_KEY_INPUTS],
                n: 0,
                gap_events: 0,
                lost: 0,
                dropped: 0,
                outage_said: false,
                fail_lines: 0,
                sent_said: false,
                broken: false,
            },
            imed: None,
        }
    }
}

/// One frame of entries on its way to windowd, and what the channel has counted.
struct Outbox {
    frame: [KeyInput; MAX_KEY_INPUTS],
    n: usize,
    /// HID events that never reached inputd (sequence gaps).
    gap_events: u32,
    /// Key entries windowd could not be handed (a dead route).
    lost: u32,
    /// Key entries no compositor ran to take (its restart, RFC-0112).
    dropped: u32,
    /// The current outage was named.
    outage_said: bool,
    fail_lines: u32,
    sent_said: bool,
    /// A send failed: the route is re-made before the next one.
    broken: bool,
}

impl Outbox {
    fn push(&mut self, entry: KeyInput, client: Option<&KernelClient>) {
        if self.n == MAX_KEY_INPUTS {
            self.flush(client);
        }
        self.frame[self.n] = entry;
        self.n += 1;
    }

    /// The filled entries to windowd in one blocking send.
    fn flush(&mut self, client: Option<&KernelClient>) {
        let n = core::mem::take(&mut self.n);
        if n == 0 {
            return;
        }
        let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
        let sent = match (client, encode_push_key_input(&self.frame[..n], &mut buf)) {
            (Some(client), Some(len)) => client.send(&buf[..len], Wait::Blocking),
            _ => Err(nexus_ipc::IpcError::Unsupported),
        };
        self.broken = sent.is_err();
        match sent {
            Ok(()) => {
                self.outage_said = false;
                if !core::mem::replace(&mut self.sent_said, true) {
                    let _ = nexus_abi::trace_line("inputd: key channel to windowd open");
                }
            }
            // The compositor is gone (RFC-0112: its restart) — no window can take a key until
            // its app rejoins the new one, and a key held for later could land in another window.
            Err(nexus_ipc::IpcError::Disconnected)
            | Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => {
                self.dropped = self.dropped.saturating_add(n as u32);
                if !core::mem::replace(&mut self.outage_said, true) {
                    let _ = debug_println("inputd: keys dropped (no compositor)");
                }
            }
            Err(_) => {
                self.lost = self.lost.saturating_add(n as u32);
                self.say(&format!("inputd: FAIL key send (lost={})", self.lost));
            }
        }
    }

    fn say(&mut self, line: &str) {
        if self.fail_lines < FAIL_LINES_MAX {
            self.fail_lines += 1;
            let _ = debug_println(line);
        }
    }
}

impl LiveRouteRuntime {
    /// RFC-0113 §1: the batch continues its device's stream, or the gap is counted and named.
    pub(super) fn check_stream(&mut self, device: u16, seq_first: u32, len: usize) {
        let out = &mut self.keys.out;
        match self.keys.streams.check(device, seq_first, len as u32) {
            SeqCheck::InOrder => {}
            SeqCheck::Gap(missing) => {
                out.gap_events = out.gap_events.saturating_add(missing);
                let total = out.gap_events;
                out.say(&format!(
                    "inputd: FAIL input gap (device={device} missing={missing} total={total})"
                ));
            }
            SeqCheck::Restart => {
                out.say(&format!("inputd: input stream restart (device={device})"))
            }
        }
    }

    /// The keys in the dispatch log — a batch just applied, or a repeat just due — to windowd, in
    /// order, in as few frames as they fit.
    pub(super) fn send_keys(&mut self) {
        let source = KeySource::of(self.input.layout());
        let Self { input, keys, windowd_client, .. } = self;
        if keys.out.broken || windowd_client.is_none() {
            // The declared leg (init pins it before inputd runs): no route ask in the key path.
            *windowd_client =
                KernelClient::new_with_slots(topo::WINDOWD.send, topo::WINDOWD.recv).ok();
            keys.out.broken = false;
        }
        let client = windowd_client.as_ref();
        let KeyChannel { translator, out, .. } = keys;
        for dispatch in input.recent_dispatches() {
            let InputDispatch::Keyboard { output, repeated, seq, t_ns, .. } = dispatch else {
                continue;
            };
            let key = ChannelKey { output: *output, repeated: *repeated, seq: *seq, t_ns: *t_ns };
            translator.feed(source, key, &mut |entry| out.push(entry, client));
        }
        out.flush(client);
    }

    /// The held key's repeat, when it is due (the pacing timer woke for it).
    pub(super) fn tick_key_repeat(&mut self) {
        let now = nsec().unwrap_or(0);
        if self.input.next_repeat_deadline_ns().is_none_or(|due| now < due) {
            return;
        }
        if self.input.tick_repeat_in_place(now).is_ok() {
            self.send_keys();
        }
    }

    /// An applied keymap change: a pending accent goes with the old layout, and the composition
    /// engine follows (`OP_SET_LAYOUT` to imed, RFC-0075 Phase 3). A blocking send — a lost
    /// switch would leave imed composing for the old layout; imed never waits on inputd.
    pub(super) fn layout_changed(&mut self, layout: &str) {
        use nexus_wire::imed as ime_wire;
        self.keys.translator.reset();
        if self.keys.imed.is_none() {
            self.keys.imed = KernelClient::new_for("imed").ok();
        }
        let Some(client) = self.keys.imed.as_ref() else {
            return;
        };
        let mut frame = [0u8; 16];
        let Some(n) = ime_wire::encode_set_layout(layout, &mut frame) else {
            return;
        };
        if client.send(&frame[..n], Wait::Blocking).is_err() {
            self.keys.imed = None; // re-routed on the next change
            self.keys.out.say("inputd: FAIL layout relay to imed");
        }
    }
}
