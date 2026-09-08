// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A facade stream as the relay's `Stream`: every read/write is
//! one bounded netstackd RPC (≤ 480 bytes), `WouldBlock` is passed through
//! so the relay's per-turn budget stays honest, and dropping the handle
//! closes the stream at the facade (a released link never leaks a socket).
//! OWNERS: @runtime
//! STATUS: Experimental

use crate::forward::{IoError, Stream};

use super::netclient::{self, NetErr};

pub(crate) struct NetStream {
    id: u32,
}

impl NetStream {
    pub(crate) const fn new(id: u32) -> Self {
        Self { id }
    }

    pub(crate) fn id(&self) -> u32 {
        self.id
    }
}

fn map(err: NetErr) -> IoError {
    match err {
        NetErr::WouldBlock | NetErr::Timeout => IoError::WouldBlock,
        NetErr::Deny | NetErr::Closed | NetErr::Io => IoError::Closed,
    }
}

impl Stream for NetStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        netclient::read(self.id, buf).map_err(map)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        netclient::write(self.id, buf).map_err(map)
    }
}

impl Drop for NetStream {
    fn drop(&mut self) {
        netclient::close(self.id);
    }
}
