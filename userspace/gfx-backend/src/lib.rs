// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! GfxBackend trait + CpuMockBackend.

#![cfg_attr(target_os = "none", no_std)]

extern crate alloc;

pub mod cpu_mock;
pub mod error;
pub mod traits;
pub mod types;

pub use cpu_mock::CpuMockBackend;
pub use error::GfxError;
pub use traits::GfxBackend;
pub use types::{Rect, ResourceId};
