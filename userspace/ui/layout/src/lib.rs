// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]
#![allow(clippy::too_many_arguments)]

extern crate alloc;

pub mod engine;
pub mod error;
#[cfg(test)]
mod engine_tests;

pub use engine::{LayoutBox, LayoutEngine, LayoutResult};
pub use error::LayoutError;
