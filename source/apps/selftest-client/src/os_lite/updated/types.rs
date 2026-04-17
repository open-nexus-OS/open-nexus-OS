//! TASK-0023B P2-14: shared types and constants for the updated submodule.
//!
//! Hosts the test-key-signed system bundle bytes and the A/B `SlotId` enum
//! used across stage / switch / status / health helpers. Re-exported from
//! `super::*` so call-sites are unaffected.

// SECURITY: bring-up test system-set signed with a test key (NOT production custody).
pub(crate) const SYSTEM_TEST_NXS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/system-test.nxs"));

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotId {
    A,
    B,
}
