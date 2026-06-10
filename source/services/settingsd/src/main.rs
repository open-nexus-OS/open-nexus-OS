// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Settings daemon entrypoint – delegates to the userspace settings library
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (via userspace/settings)
//! ADR: docs/adr/0011-settings-architecture.md

fn main() {
    settingsd::run();
    println!("settingsd: ready");
}
