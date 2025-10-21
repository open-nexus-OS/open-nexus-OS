#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

//! Minimal init process responsible for launching core services and emitting
//! deterministic UART markers for the OS test harness.
//!
//! The library exposes two backends selected via `nexus_env` and the
//! `os-lite` feature. Host builds keep the original std runtime while the OS
//! variant uses a cooperative bootstrap stub that will gain capabilities in
//! later stages of the migration.

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
