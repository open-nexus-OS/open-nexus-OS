//! Bundle manager domain logic shared between host tests and the OS daemon.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

// Feature validation: ensure exactly one backend is selected
#[cfg(all(feature = "backend-host", feature = "backend-os"))]
compile_error!("Enable only one of `backend-host` or `backend-os`.");

pub mod cli;
pub mod manifest;

pub use cli::{execute, help, run_with, AbilityRegistrar};
/// Bundle manifest error type.
pub use manifest::Error;
/// Bundle manifest model and parser.
pub use manifest::Manifest;