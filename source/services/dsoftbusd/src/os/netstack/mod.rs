//! CONTEXT: Netstack RPC adapter helpers for cross-VM dsoftbusd path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod ids;
pub(crate) mod rpc;
pub(crate) mod stream_io;
pub(crate) mod validate;

pub(crate) use ids::{SessionId, UdpSocketId};
pub(crate) use rpc::{next_nonce, rpc_nonce};
pub(crate) use stream_io::{
    stream_read_exact, stream_write_all, tcp_listen, udp_bind, udp_send_to, CrossVmTransport,
    STATUS_IO, STATUS_WOULD_BLOCK,
};
