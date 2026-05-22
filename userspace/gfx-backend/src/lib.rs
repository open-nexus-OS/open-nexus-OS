// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! GfxBackend trait + CpuMockBackend.

extern crate alloc;

pub mod traits;
pub mod cpu_mock;
pub mod error;
pub mod types;

pub use traits::GfxBackend;
pub use cpu_mock::CpuMockBackend;
pub use error::GfxError;
pub use types::{Rect, ResourceId};
