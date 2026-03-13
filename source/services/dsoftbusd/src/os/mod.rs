//! CONTEXT: Internal OS-only module boundaries for dsoftbusd refactor slices.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod entry;
pub(crate) mod entry_pure;
pub(crate) mod discovery;
pub(crate) mod gateway;
pub(crate) mod netstack;
pub(crate) mod observability;
pub(crate) mod session;
pub(crate) mod service_clients;
