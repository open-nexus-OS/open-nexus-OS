//! Bundle manager domain logic shared between host tests and the OS daemon.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(all(feature = "backend-host", feature = "backend-os"))]
compile_error!("Choose exactly one backend feature.");

#[cfg(not(any(feature = "backend-host", feature = "backend-os")))]
compile_error!("Select a backend feature.");

pub mod cli;
pub mod manifest;

pub use cli::{execute, help, run_with, AbilityRegistrar};
