// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Virtual memory primitives for Sv39 address spaces.

pub mod address_space;
pub mod page_table;

pub use address_space::{AddressSpace, AddressSpaceError, AddressSpaceManager, AsHandle};
pub use page_table::{MapError, PageFlags, PageTable, PAGE_SIZE};

#[cfg(test)]
mod tests;
