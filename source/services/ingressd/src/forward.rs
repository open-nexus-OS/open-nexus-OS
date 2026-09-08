// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded bidirectional stream relay (RFC-0092 §3 forwarding):
//! one `Link` copies bytes between an accepted peer stream and the loopback
//! backend stream through two fixed 4 KiB windows, moving at most `budget`
//! bytes per `pump` so the gateway loop stays reactive; a `Relay` holds at
//! most 16 links per exposure (the 17th is refused, `Reason::Limit`, never
//! queued). Streams are abstract (`Stream`) — the OS host binds them to the
//! facade's TCP handles, host tests to in-memory pipes.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: unit tests below, tests/ingress_host/ (`expose_allow_end_to_end`)

use crate::wire::Reason;

/// Bytes buffered per direction.
pub const WINDOW_BYTES: usize = 4096;
/// Concurrent forwarded connections per exposure.
pub const MAX_LINKS_PER_EXPOSE: usize = 16;

/// Non-blocking stream errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    /// Nothing to read / no room to write right now.
    WouldBlock,
    /// The peer is gone.
    Closed,
}

/// A non-blocking byte stream. `read` returning `Ok(0)` is end-of-stream.
pub trait Stream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError>;
}

#[derive(Debug, Clone, Copy)]
struct Window {
    buf: [u8; WINDOW_BYTES],
    head: usize,
    len: usize,
    /// Source reached end-of-stream; drain what is buffered, then half-close.
    eof: bool,
}

impl Window {
    const EMPTY: Self = Self { buf: [0; WINDOW_BYTES], head: 0, len: 0, eof: false };

    fn drained(&self) -> bool {
        self.eof && self.len == 0
    }
}

/// Link state after a pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Open,
    Closed,
}

/// One forwarded connection: `a` (peer) ↔ `b` (backend).
pub struct Link<A: Stream, B: Stream> {
    pub a: A,
    pub b: B,
    a2b: Window,
    b2a: Window,
    closed: bool,
}

impl<A: Stream, B: Stream> Link<A, B> {
    /// Fresh link, nothing buffered.
    pub fn new(a: A, b: B) -> Self {
        Self { a, b, a2b: Window::EMPTY, b2a: Window::EMPTY, closed: false }
    }

    /// Moves up to `budget` bytes per direction (read + write counted) and
    /// reports the link state. Both directions drained, or any stream
    /// error other than `WouldBlock`, closes the link.
    pub fn pump(&mut self, budget: usize) -> LinkState {
        if self.closed {
            return LinkState::Closed;
        }
        let ok_ab = pump_dir(&mut self.a, &mut self.b, &mut self.a2b, budget);
        let ok_ba = pump_dir(&mut self.b, &mut self.a, &mut self.b2a, budget);
        if !ok_ab || !ok_ba || (self.a2b.drained() && self.b2a.drained()) {
            self.closed = true;
            return LinkState::Closed;
        }
        LinkState::Open
    }

    /// `true` once closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// One direction: fill the window from `src`, drain it into `dst`.
/// Returns `false` on a hard stream error.
fn pump_dir(src: &mut impl Stream, dst: &mut impl Stream, w: &mut Window, budget: usize) -> bool {
    let mut moved = 0usize;
    // Compact so the free region is contiguous at the tail.
    if w.head > 0 {
        w.buf.copy_within(w.head..w.head + w.len, 0);
        w.head = 0;
    }
    if !w.eof && w.len < WINDOW_BYTES && moved < budget {
        let room = (WINDOW_BYTES - w.len).min(budget - moved);
        match src.read(&mut w.buf[w.len..w.len + room]) {
            Ok(0) => w.eof = true,
            Ok(n) => {
                w.len += n;
                moved += n;
            }
            Err(IoError::WouldBlock) => {}
            Err(IoError::Closed) => return false,
        }
    }
    if w.len > 0 && moved < budget {
        let take = w.len.min(budget - moved);
        match dst.write(&w.buf[w.head..w.head + take]) {
            Ok(n) => {
                w.head += n;
                w.len -= n;
                if w.len == 0 {
                    w.head = 0;
                }
            }
            Err(IoError::WouldBlock) => {}
            Err(IoError::Closed) => return false,
        }
    }
    true
}

/// Bounded link table of one exposure.
pub struct Relay<A: Stream, B: Stream, const N: usize = MAX_LINKS_PER_EXPOSE> {
    links: [Option<Link<A, B>>; N],
}

impl<A: Stream, B: Stream, const N: usize> Default for Relay<A, B, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Stream, B: Stream, const N: usize> Relay<A, B, N> {
    /// Empty table.
    pub fn new() -> Self {
        Self { links: [const { None }; N] }
    }

    /// `true` while another link fits.
    pub fn has_room(&self) -> bool {
        self.links.iter().any(Option::is_none)
    }

    /// Adds a link; a full table refuses (`Reason::Limit`) and drops the
    /// link, which closes both ends (check [`Self::has_room`] to decide
    /// before accepting).
    pub fn attach(&mut self, link: Link<A, B>) -> Result<usize, Reason> {
        match self.links.iter().position(Option::is_none) {
            Some(i) => {
                self.links[i] = Some(link);
                Ok(i)
            }
            None => Err(Reason::Limit),
        }
    }

    /// Pumps every link once; closed links are released. Returns the
    /// number still open.
    pub fn pump_all(&mut self, budget: usize) -> usize {
        let mut open = 0;
        for slot in &mut self.links {
            let closed = matches!(slot.as_mut().map(|l| l.pump(budget)), Some(LinkState::Closed));
            if closed {
                *slot = None;
            } else if slot.is_some() {
                open += 1;
            }
        }
        open
    }

    /// Links currently attached.
    pub fn active(&self) -> usize {
        self.links.iter().filter(|l| l.is_some()).count()
    }

    /// Borrow link `i`.
    pub fn link(&self, i: usize) -> Option<&Link<A, B>> {
        self.links.get(i).and_then(Option::as_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream whose reads come from `inbox` and writes land in `outbox`.
    struct Pipe {
        inbox: [u8; 64],
        inbox_len: usize,
        outbox: [u8; 64],
        outbox_len: usize,
        eof: bool,
    }

    impl Pipe {
        fn with(data: &[u8]) -> Self {
            let mut p = Self {
                inbox: [0; 64],
                inbox_len: data.len(),
                outbox: [0; 64],
                outbox_len: 0,
                eof: true,
            };
            p.inbox[..data.len()].copy_from_slice(data);
            p
        }
    }

    impl Stream for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
            if self.inbox_len == 0 {
                return if self.eof { Ok(0) } else { Err(IoError::WouldBlock) };
            }
            let n = self.inbox_len.min(buf.len());
            buf[..n].copy_from_slice(&self.inbox[..n]);
            self.inbox.copy_within(n..self.inbox_len, 0);
            self.inbox_len -= n;
            Ok(n)
        }

        fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
            let n = buf.len().min(64 - self.outbox_len);
            if n == 0 {
                return Err(IoError::WouldBlock);
            }
            self.outbox[self.outbox_len..self.outbox_len + n].copy_from_slice(&buf[..n]);
            self.outbox_len += n;
            Ok(n)
        }
    }

    #[test]
    fn bytes_cross_both_ways_within_budget() {
        let mut link = Link::new(Pipe::with(b"hello"), Pipe::with(b"world!"));
        // Budget 3 per direction per pump: 5 and 6 bytes need several turns.
        assert_eq!(link.pump(3), LinkState::Open);
        assert!(link.b.outbox_len <= 3 && link.a.outbox_len <= 3);
        let mut turns = 0;
        while link.pump(3) == LinkState::Open {
            turns += 1;
            assert!(turns < 16, "relay must terminate");
        }
        assert_eq!(&link.b.outbox[..link.b.outbox_len], b"hello");
        assert_eq!(&link.a.outbox[..link.a.outbox_len], b"world!");
        assert!(link.is_closed());
    }

    #[test]
    fn relay_refuses_the_seventeenth_link() {
        let mut relay: Relay<Pipe, Pipe, 2> = Relay::new();
        assert_eq!(relay.attach(Link::new(Pipe::with(b""), Pipe::with(b""))), Ok(0));
        assert_eq!(relay.attach(Link::new(Pipe::with(b""), Pipe::with(b""))), Ok(1));
        assert!(!relay.has_room());
        assert_eq!(relay.attach(Link::new(Pipe::with(b""), Pipe::with(b""))), Err(Reason::Limit));
        // Empty links (EOF both ways) drain on the first pump and free slots.
        assert_eq!(relay.pump_all(16), 0);
        assert_eq!(relay.active(), 0);
    }
}
