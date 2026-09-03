// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Where a boot service's image comes from (TASK-0321 P2,
//! RFC-0089 §12.3, ADR-0060). Services are either EMBEDDED in init-lite's
//! generated table or live as `.nxb` bundles on the verified system
//! volume, spawned by init from an ELF that bundlemgrd served and
//! verified. The SSOT of the volume set is `scripts/system-volume-
//! services.txt` (the same file `scripts/build.sh` uses to emit the
//! bundles and to drop them from `INIT_LITE_SERVICE_LIST`) — included at
//! compile time, so init and the build can never disagree. CORE (wave-1)
//! services can never be on the volume: recovery and safe boots must not
//! depend on it (`boot_graph::in_core`), enforced by a host test.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`core_services_never_on_volume`,
//!   list parse, pilot present)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

/// The build-side SSOT, verbatim (comments + blank lines are skipped).
const VOLUME_SERVICES_TXT: &str = include_str!("../../../../scripts/system-volume-services.txt");

/// Hard cap on the volume service set (init holds one VMO mapping per
/// volume service for the process lifetime — bounded cap-table use).
pub const MAX_VOLUME_SERVICES: usize = 16;

/// Iterates the volume service names in file order.
pub fn volume_services() -> impl Iterator<Item = &'static str> {
    VOLUME_SERVICES_TXT
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|name| !name.is_empty())
        .take(MAX_VOLUME_SERVICES)
}

/// `true` if `name` is spawned from the system volume (P2+), i.e. it is NOT
/// expected in the embedded image table.
pub fn is_volume_service(name: &str) -> bool {
    volume_services().any(|s| s == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parses_and_contains_the_pilot() {
        let names: alloc::vec::Vec<&str> = volume_services().collect();
        assert!(names.contains(&"metricsd"), "P2 pilot must be on the volume: {names:?}");
        for n in &names {
            assert!(
                n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "service name must be a plain identifier: {n:?}"
            );
        }
    }

    #[test]
    fn core_services_never_on_volume() {
        // ADR-0060 D4: the wave-1 CORE graph (and the recovery/safe boots
        // that rely on it) must never depend on a system volume.
        for n in volume_services() {
            assert!(!crate::boot_graph::in_core(n), "CORE service {n} must stay embedded");
        }
        assert!(!is_volume_service("bundlemgrd"));
        assert!(!is_volume_service("virtioblkd"));
    }
}
