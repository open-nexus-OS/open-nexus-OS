// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Notification daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (via userspace/notif)
//! ADR: docs/adr/0013-notification-architecture.md

fn main() {
    notif::run();
    println!("notifd: ready");
}
