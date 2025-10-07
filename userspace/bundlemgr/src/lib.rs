//! Bundle manager domain logic shared between host tests and the OS daemon.
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
