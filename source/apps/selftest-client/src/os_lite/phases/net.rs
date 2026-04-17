//! Phase: net (extracted in Cut P2-11 of TASK-0023B).
//!
//! Owns the netstackd-anchored slice:
//!   netstackd_local_addr() -> ctx.local_ip + ctx.os2vm classification +
//!   TASK-0004 ICMP ping proof (single-VM mode only) +
//!   TASK-0003 DSoftBus OS transport bring-up via netstackd facade
//!   (single-VM mode only; emits the QUIC-subset connect/ping markers).
//!
//! Marker order and marker strings are byte-identical to the pre-cut body.
//!
//! `ctx.local_ip` and `ctx.os2vm` are written here and read by the remote
//! phase (P2-12) to gate the cross-VM proxy proof.

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
            emit_line("SELFTEST: icmp ping ok");
        } else {
            emit_line("SELFTEST: icmp ping FAIL");
        }
    }

    // TASK-0003: DSoftBus OS transport bring-up via netstackd facade.
    // Under os2vm mode, we rely on real cross-VM discovery+sessions instead (TASK-0005),
    // so skip this local-only probe to avoid false FAIL markers and long waits.
    if !ctx.os2vm {
        if dsoftbus::quic_os::dsoftbus_os_transport_probe().is_ok() {
            emit_line("SELFTEST: dsoftbus os connect ok");
            emit_line("SELFTEST: dsoftbus ping ok");
        } else {
            emit_line("SELFTEST: dsoftbus os connect FAIL");
            emit_line("SELFTEST: dsoftbus ping FAIL");
        }
    }

    Ok(())
}
