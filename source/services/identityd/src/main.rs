// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Identity daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0006-device-identity-architecture.md

fn main() -> ! {
    identityd::touch_schemas();
    if let Err(err) = identityd::service_main_loop(identityd::ReadyNotifier::new(|| ())) {
        eprintln!("identityd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
