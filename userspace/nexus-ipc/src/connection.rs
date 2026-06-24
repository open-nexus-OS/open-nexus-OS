// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: One reusable typed request/reply IPC client — the single correct
//! cross-service call path (RFC-0066).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 tests
//!
//! One reusable typed request/reply client (RFC-0066).
//!
//! This is the single correct cross-service call path — the OHOS proxy / Fuchsia
//! channel / Apple XPC-connection equivalent. Every client (abilitymgr, windowd,
//! …) calls a service through a [`Connection`] instead of hand-rolling the
//! nonce-correlated CAP_MOVE request/reply at each call site (the copy-paste that
//! made the chain fragile).
//!
//! The transport is abstracted: the OS supplies a kernel CAP_MOVE transport, host
//! tests supply an in-memory one — so the correlation logic is proven on the host
//! without booting.

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

use crate::reqrep::{recv_match_bounded, NonceGen, ReplyBuffer};
use crate::{Client, Result};

/// The send/reply transport a [`Connection`] runs over.
///
/// Implementors: a kernel CAP_MOVE transport (OS), an in-memory loopback (tests).
pub trait Transport {
    /// The inbox where correlated replies arrive.
    type Inbox: Client;

    /// Sends one request frame to the service (OS: moves a reply capability so the
    /// service can reply to this caller; loopback: enqueues to the peer).
    fn send_request(&self, frame: &[u8]) -> Result<()>;

    /// The reply inbox this connection receives correlated replies on.
    fn reply_inbox(&self) -> &Self::Inbox;
}

/// A typed request/reply connection to one service.
///
/// `PENDING` bounds the out-of-order reply buffer; `MAX_FRAME` bounds a reply size.
pub struct Connection<T: Transport, const PENDING: usize, const MAX_FRAME: usize> {
    transport: T,
    pending: ReplyBuffer<PENDING, MAX_FRAME>,
    nonces: NonceGen,
}

impl<T: Transport, const PENDING: usize, const MAX_FRAME: usize>
    Connection<T, PENDING, MAX_FRAME>
{
    /// Creates a connection over `transport`, with nonces starting at `nonce_start`.
    pub fn new(transport: T, nonce_start: u64) -> Self {
        Self { transport, pending: ReplyBuffer::new(), nonces: NonceGen::new(nonce_start) }
    }

    /// Performs one request → reply round trip, bounded by `max_iters` receive
    /// attempts (the caller's deadline/backoff policy lives in `max_iters`).
    ///
    /// `build(nonce)` produces the request frame stamped with the correlation
    /// `nonce`; `extract_nonce(reply)` pulls the nonce back out of a reply frame so
    /// out-of-order replies are buffered and the matching one is returned.
    pub fn call(
        &mut self,
        max_iters: usize,
        build: impl FnOnce(u64) -> Vec<u8>,
        extract_nonce: impl Fn(&[u8]) -> Option<u64>,
    ) -> Result<Vec<u8>> {
        let nonce = self.nonces.next_nonce();
        let frame = build(nonce);
        self.transport.send_request(&frame)?;
        recv_match_bounded(
            self.transport.reply_inbox(),
            &mut self.pending,
            nonce,
            max_iters,
            extract_nonce,
        )
    }

    /// Borrows the underlying transport (e.g. to inspect connection state).
    pub fn transport(&self) -> &T {
        &self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IpcError, Wait};
    use core::cell::RefCell;

    /// In-memory inbox: replies are popped FIFO; empty yields `WouldBlock`.
    struct MemInbox {
        frames: RefCell<Vec<Vec<u8>>>,
    }
    impl Client for MemInbox {
        fn send(&self, _frame: &[u8], _wait: Wait) -> Result<()> {
            Ok(())
        }
        fn recv(&self, _wait: Wait) -> Result<Vec<u8>> {
            let mut f = self.frames.borrow_mut();
            if f.is_empty() {
                Err(IpcError::WouldBlock)
            } else {
                Ok(f.remove(0))
            }
        }
    }

    /// A synchronous mock service: on each request it computes reply frames via
    /// `responder` and stashes them in the inbox (models a service that has
    /// already replied by the time the caller receives).
    struct MockService {
        inbox: MemInbox,
        responder: fn(&[u8]) -> Vec<Vec<u8>>,
    }
    impl Transport for MockService {
        type Inbox = MemInbox;
        fn send_request(&self, frame: &[u8]) -> Result<()> {
            for reply in (self.responder)(frame) {
                self.inbox.frames.borrow_mut().push(reply);
            }
            Ok(())
        }
        fn reply_inbox(&self) -> &MemInbox {
            &self.inbox
        }
    }

    fn nonce_le(frame: &[u8]) -> Option<u64> {
        frame.get(0..8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }

    fn echo_nonce(req: &[u8]) -> Vec<Vec<u8>> {
        // Reply == the request's nonce (first 8 bytes) + a payload byte.
        let mut reply = req[0..8].to_vec();
        reply.push(0xAB);
        Vec::from([reply])
    }

    #[test]
    fn call_correlates_reply_by_nonce() {
        let svc = MockService { inbox: MemInbox { frames: RefCell::new(Vec::new()) }, responder: echo_nonce };
        let mut conn: Connection<_, 4, 32> = Connection::new(svc, 1);
        let reply = conn
            .call(8, |nonce| nonce.to_le_bytes().to_vec(), nonce_le)
            .expect("reply");
        assert_eq!(nonce_le(&reply), Some(1));
        assert_eq!(reply[8], 0xAB);
        // Second call uses the next nonce.
        let reply2 = conn.call(8, |nonce| nonce.to_le_bytes().to_vec(), nonce_le).unwrap();
        assert_eq!(nonce_le(&reply2), Some(2));
    }

    #[test]
    fn call_buffers_out_of_order_replies() {
        // The service stashes a stale (wrong-nonce) frame before the real one.
        fn stale_then_match(req: &[u8]) -> Vec<Vec<u8>> {
            let mut stale = 999u64.to_le_bytes().to_vec();
            stale.push(0x00);
            let mut good = req[0..8].to_vec();
            good.push(0xAB);
            Vec::from([stale, good])
        }
        let svc = MockService { inbox: MemInbox { frames: RefCell::new(Vec::new()) }, responder: stale_then_match };
        let mut conn: Connection<_, 4, 32> = Connection::new(svc, 7);
        let reply = conn.call(8, |n| n.to_le_bytes().to_vec(), nonce_le).expect("reply");
        assert_eq!(nonce_le(&reply), Some(7));
    }

    #[test]
    fn call_times_out_when_no_matching_reply() {
        fn no_match(_req: &[u8]) -> Vec<Vec<u8>> {
            let mut wrong = 12345u64.to_le_bytes().to_vec();
            wrong.push(0);
            Vec::from([wrong])
        }
        let svc = MockService { inbox: MemInbox { frames: RefCell::new(Vec::new()) }, responder: no_match };
        let mut conn: Connection<_, 4, 32> = Connection::new(svc, 1);
        let r = conn.call(4, |n| n.to_le_bytes().to_vec(), nonce_le);
        assert!(matches!(r, Err(IpcError::Timeout)));
    }
}
