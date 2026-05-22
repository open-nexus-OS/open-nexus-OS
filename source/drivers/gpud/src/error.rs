// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDriverError {
    DeviceNotFound,
    MmioFault,
    CommandRejected,
    ResourceExhausted,
    Unsupported,
    InvalidArgument,
}
