// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host has NO build-time payload. Program bytes come exclusively
//! from bundlemgrd's registry (GET_PAYLOAD → VMO) — the embedded fallback was
//! deleted (separation of concerns: registry owns programs, the runtime only
//! executes them; a missing payload fails LOUD + VISIBLE instead of silently
//! running a baked-in program).
//! OWNERS: @ui @runtime
//! STATUS: Functional

fn main() {}
