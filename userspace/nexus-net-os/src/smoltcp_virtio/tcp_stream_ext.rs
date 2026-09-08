// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OsTcpStream` inherent helpers (readiness spin, close/remove,
//! state probes, remote endpoint) — a child module of `smoltcp_virtio` so
//! it shares the parent's private socket-set access (module-size ratchet
//! split; RFC-0092 `OP_PEER_ADDR` reads `remote_endpoint`).
//! OWNERS: @runtime
//! STATUS: Experimental

use nexus_net::NetSocketAddrV4;
use smoltcp::wire::IpAddress;

use super::{poll_inner_once, OsTcpStream};

impl OsTcpStream {
    /// The connected remote endpoint (`None` while closed/listening) — the
    /// accept-side identity a gateway judges (RFC-0092 `OP_PEER_ADDR`).
    pub fn remote_endpoint(&self) -> Option<NetSocketAddrV4> {
        let inner = self.inner.borrow();
        let sock = inner.sockets.get::<smoltcp::socket::tcp::Socket>(self.handle);
        let ep = sock.remote_endpoint()?;
        match ep.addr {
            IpAddress::Ipv4(ip) => Some(NetSocketAddrV4::new(ip.0, ep.port)),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    pub fn wait_writable_bounded(&mut self, max_polls: u32) -> bool {
        for _ in 0..=max_polls {
            let mut inner = self.inner.borrow_mut();
            let now = inner.now;
            {
                let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
                match sock.state() {
                    smoltcp::socket::tcp::State::Closed | smoltcp::socket::tcp::State::Listen => {
                        return false
                    }
                    _ => {}
                }
                if sock.may_send() {
                    return true;
                }
            }
            poll_inner_once(&mut inner, now.saturating_add(1));
        }
        false
    }

    /// Close and dispose the underlying socket handle from the shared socket set.
    ///
    /// Use this when a connect attempt must be abandoned before the stream is handed out.
    pub fn close_and_remove(self) {
        let mut inner = self.inner.borrow_mut();
        {
            let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
            sock.close();
        }
        inner.sockets.remove(self.handle);
    }

    /// Returns true when the stream is no longer in a connect/connected state.
    pub fn is_closed_or_listen(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        matches!(
            sock.state(),
            smoltcp::socket::tcp::State::Closed | smoltcp::socket::tcp::State::Listen
        )
    }
}
