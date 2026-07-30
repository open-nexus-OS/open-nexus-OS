// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The `svc.<service>.<method>` surface lookup — GENERATED platform table +
//! the std-only app-local additions (`native/surface.toml`). Split out of
//! `registry.rs` (structure ratchet); `registry` re-exports the public API.

// ------------------------------------------------------------ svc surface
// GENERATED from the IDL SSOT (tools/nexus-idl/schemas/dsl_services.capnp):
// `SvcSig` + `SVC_SURFACE`. The checker's unknown-service/method/arity
// diagnostics derive from the same file the app-host routes against.
include!(concat!(env!("OUT_DIR"), "/svc_surface.rs"));

/// Result of looking up `svc.<service>.<method>` against the surface.
pub enum SvcLookup {
    /// Known method; the checker validates the positional arity.
    Found {
        arity: usize,
    },
    UnknownService,
    UnknownMethod,
}

/// App-local additions to the platform surface (TASK-0081 C1): the methods
/// of THIS app's `native/` companion (and, later, consumed app exports),
/// declared once in `native/surface.toml` and installed for the duration of
/// one project check (`crate::compile_project_dir`). std-only: in-system
/// no_std lint runs see the platform surface alone.
#[cfg(feature = "std")]
static APP_SURFACE: std::sync::Mutex<Vec<(String, String, usize)>> =
    std::sync::Mutex::new(Vec::new());

/// Installs the app-local surface (service, method, positional arity).
/// Callers hold [`app_surface_guard`] around the whole check to keep
/// parallel project builds from cross-contaminating.
#[cfg(feature = "std")]
pub(crate) fn set_app_surface(entries: Vec<(String, String, usize)>) {
    if let Ok(mut surface) = APP_SURFACE.lock() {
        *surface = entries;
    }
}

/// Serializes project checks that install an app-local surface.
#[cfg(feature = "std")]
pub(crate) fn app_surface_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[must_use]
pub fn svc_method(service: &str, method: &str) -> SvcLookup {
    let mut service_exists = false;
    for sig in SVC_SURFACE {
        if sig.service == service {
            service_exists = true;
            if sig.method == method {
                return SvcLookup::Found { arity: sig.args.len() };
            }
        }
    }
    #[cfg(feature = "std")]
    if let Ok(surface) = APP_SURFACE.lock() {
        for (svc, m, arity) in surface.iter() {
            if svc == service {
                service_exists = true;
                if m == method {
                    return SvcLookup::Found { arity: *arity };
                }
            }
        }
    }
    if service_exists {
        SvcLookup::UnknownMethod
    } else {
        SvcLookup::UnknownService
    }
}
