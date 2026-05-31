// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service spawn logic — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

use crate::os_payload::{InitError, ServiceImage};

/// Spawn a single service image via `exec_v2`. Returns the child PID.
#[inline]
pub(crate) fn spawn_service(image: &ServiceImage, probes_enabled: bool) -> Result<u32, InitError> {
    if image.elf.is_empty() {
        return Err(InitError::MissingElf);
    }
    let stack_pages = image.stack_pages.max(1) as usize;
    let pid = nexus_abi::exec_v2(image.elf, stack_pages, image.global_pointer, image.name)
        .map_err(InitError::Abi)?;
    // Suppress unused warning when probes are compiled out
    let _ = probes_enabled;
    Ok(pid)
}

/// Spawn with debug probe output (when probes are enabled).
pub(crate) fn spawn_service_with_probe(
    image: &ServiceImage,
    probes_enabled: bool,
) -> Result<u32, InitError> {
    use crate::os_payload::{debug_write_byte, debug_write_bytes, debug_write_str};

    if image.elf.is_empty() {
        return Err(InitError::MissingElf);
    }
    let stack_pages = image.stack_pages.max(1) as usize;
    if probes_enabled {
        debug_write_bytes(b"!exec call name=");
        debug_write_str(image.name);
        debug_write_byte(b'\n');
    }
    let pid = nexus_abi::exec_v2(image.elf, stack_pages, image.global_pointer, image.name)
        .map_err(InitError::Abi)?;
    if probes_enabled {
        debug_write_bytes(b"!exec ret\n");
    }
    Ok(pid)
}
