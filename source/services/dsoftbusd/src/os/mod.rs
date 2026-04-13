//! CONTEXT: Internal OS-only module boundaries for dsoftbusd refactor slices.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod discovery;
pub(crate) mod entry;
pub(crate) mod entry_pure;
pub(crate) mod gateway;
pub(crate) mod netstack;
pub(crate) mod observability;
pub(crate) mod service_clients;
pub(crate) mod session;

// Reuse the canonical mux-v2 contract implementation in OS paths to avoid
// marker-only proofs diverging from host contract semantics.
#[path = "../../../../../userspace/dsoftbus/src/mux_v2.rs"]
pub(crate) mod mux_v2;
