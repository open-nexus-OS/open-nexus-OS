// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 10 of 12 — net (netstackd local-addr resolution + os2vm
//!   classification, TASK-0004 ICMP ping proof, TASK-0003 DSoftBus OS
//!   transport bring-up + QUIC-subset connect/ping markers; single-VM mode
//!   only — 2-VM mode defers most network proofs to `phases::remote`).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — net / DSoftBus slice;
//!   `REQUIRE_DSOFTBUS=1` gates the QUIC subset.
//!
//! Extracted in Cut P2-11 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. `ctx.local_ip` and `ctx.os2vm` are
//! written here and read by the remote phase (P2-12) to gate the cross-VM
//! proxy proof.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::{dsoftbus, net};

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    ctx.local_ip = net::local_addr::netstackd_local_addr();
    ctx.os2vm = matches!(ctx.local_ip, Some([10, 42, 0, _]));

    // TASK-0004: ICMP ping proof via netstackd facade.
    // Under 2-VM socket/mcast backends there is no gateway, so skip deterministically.
    //
    // Note: QEMU slirp DHCP commonly assigns 10.0.2.15, which is also the deterministic static
    // fallback IP. Therefore we MUST NOT infer DHCP availability from the local IP alone.
    // Always attempt the bounded ICMP probe in single-VM mode; the harness decides whether it
    // is required (REQUIRE_QEMU_DHCP=1) based on the `net: dhcp bound` marker.
    if !ctx.os2vm {
        if net::icmp_ping::icmp_ping_probe().is_ok() {
            emit_line(crate::markers::M_SELFTEST_ICMP_PING_OK);
        } else {
            emit_line(crate::markers::M_SELFTEST_ICMP_PING_FAIL);
        }
    }

    // TASK-0003: DSoftBus OS transport bring-up via netstackd facade.
    // Under os2vm mode, we rely on real cross-VM discovery+sessions instead (TASK-0005),
    // so skip this local-only probe to avoid false FAIL markers and long waits.
    if !ctx.os2vm {
        if dsoftbus::quic_os::dsoftbus_os_transport_probe().is_ok() {
            emit_line(crate::markers::M_SELFTEST_DSOFTBUS_OS_CONNECT_OK);
            emit_line(crate::markers::M_SELFTEST_DSOFTBUS_PING_OK);
        } else {
            emit_line(crate::markers::M_SELFTEST_DSOFTBUS_OS_CONNECT_FAIL);
            emit_line(crate::markers::M_SELFTEST_DSOFTBUS_PING_FAIL);
        }
    }

    Ok(())
}
