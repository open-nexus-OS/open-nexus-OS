// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::buffer::{CommandBuffer, CommittedBuffer};
use crate::core::fence::Fence;
use crate::core::error::GfxError;

pub const DEFAULT_QUEUE_DEPTH: u8 = 2;

/// Single submission queue. submit() consumes the CommandBuffer.
#[derive(Debug, Clone)]
pub struct Queue {
    max_in_flight: u8,
    in_flight: u8,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            max_in_flight: DEFAULT_QUEUE_DEPTH,
            in_flight: 0,
        }
    }

    pub fn with_depth(max_in_flight: u8) -> Result<Self, GfxError> {
        if max_in_flight == 0 {
            return Err(GfxError::InvalidArgument);
        }
        Ok(Self {
            max_in_flight,
            in_flight: 0,
        })
    }

    pub fn submit(&mut self, cmd: CommandBuffer) -> Result<Fence, GfxError> {
        self.submit_committed(cmd.try_commit()?)
    }

    pub fn submit_committed(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        if self.in_flight >= self.max_in_flight {
            return Err(GfxError::ResourceExhausted);
        }
        cmd.validate()?;
        self.in_flight = self.in_flight.saturating_add(1);
        // The host queue model completes synchronously; real backends return their own fence.
        self.in_flight = self.in_flight.saturating_sub(1);
        Ok(Fence::new_signaled())
    }

    pub const fn in_flight(&self) -> u8 {
        self.in_flight
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}
