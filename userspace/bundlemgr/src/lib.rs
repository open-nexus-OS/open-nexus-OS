//! CONTEXT: Bundle manager domain library
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - cli::{execute, help, run_with, AbilityRegistrar}: CLI interface
//!   - manifest::{Error, Manifest}: Manifest parsing
//!   - service::{InstallRequest, InstalledBundle, Service, ServiceError}: Service layer
//!
//! DEPENDENCIES:
//!   - toml: Manifest parsing
//!   - semver: Version handling
//!   - base64: Signature encoding
//!
//! ADR: docs/adr/0009-bundle-manager-architecture.md
#![forbid(unsafe_code)]
#![deny(missing_docs)]

// Environment validation: ensure exactly one backend is selected
#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

pub mod cli;
pub mod manifest;
pub mod service;

pub use cli::{execute, help, run_with, AbilityRegistrar};
/// Bundle manifest error type.
pub use manifest::Error;
/// Bundle manifest model and parser.
pub use manifest::Manifest;
/// Service facade used by daemons and host tests.
pub use service::{InstallRequest, InstalledBundle, Service, ServiceError};
