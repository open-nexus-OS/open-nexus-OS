// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: netstackd IPC v1 wire constants
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) const MAGIC0: u8 = b'N';
pub(crate) const MAGIC1: u8 = b'S';
pub(crate) const VERSION: u8 = 1;

pub(crate) const OP_LISTEN: u8 = 1;
pub(crate) const OP_ACCEPT: u8 = 2;
pub(crate) const OP_CONNECT: u8 = 3;
pub(crate) const OP_READ: u8 = 4;
pub(crate) const OP_WRITE: u8 = 5;
pub(crate) const OP_UDP_BIND: u8 = 6;
pub(crate) const OP_UDP_SEND_TO: u8 = 7;
pub(crate) const OP_UDP_RECV_FROM: u8 = 8;
pub(crate) const OP_ICMP_PING: u8 = 9;
pub(crate) const OP_LOCAL_ADDR: u8 = 10;
pub(crate) const OP_CLOSE: u8 = 11;
pub(crate) const OP_WAIT_WRITABLE: u8 = 12;

pub(crate) const STATUS_OK: u8 = 0;
pub(crate) const STATUS_NOT_FOUND: u8 = 1;
pub(crate) const STATUS_MALFORMED: u8 = 2;
pub(crate) const STATUS_WOULD_BLOCK: u8 = 3;
pub(crate) const STATUS_IO: u8 = 4;
pub(crate) const STATUS_TIMED_OUT: u8 = 5;
