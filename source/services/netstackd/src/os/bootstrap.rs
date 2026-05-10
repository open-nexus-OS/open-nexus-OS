// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap and bring-up proof flow for netstackd networking owner path
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by netstackd QEMU markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
))]
use nexus_abi::yield_;
#[cfg(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
))]
use nexus_net::{NetError, NetSocketAddrV4, NetStack as _, UdpSocket as _};
#[cfg(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
))]
use nexus_net_os::SmoltcpVirtioNetStack;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub(crate) struct BootstrapResult {
    pub(crate) net: SmoltcpVirtioNetStack,
    pub(crate) bind_ip: [u8; 4],
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub(crate) fn bootstrap_network() -> BootstrapResult {
    let mut net = {
        let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(20_000_000_000);
        loop {
            match SmoltcpVirtioNetStack::new_default() {
                Ok(n) => break n,
                Err(err) => {
                    let now = nexus_abi::nsec().unwrap_or(0);
                    if now < deadline {
                        let _ = yield_();
                        continue;
                    }
                    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
                    {
                        let mut q =
                            nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
                        if nexus_abi::cap_query(48, &mut q).is_ok() {
                            let _ = nexus_abi::debug_println("netstackd: mmio cap48 present");
                        } else {
                            let _ = nexus_abi::debug_println("netstackd: mmio cap48 missing");
                        }
                    }
                    let (fail_label, fail_diag_code) = match err {
                        nexus_net::NetError::Unsupported => {
                            ("netstackd: net FAIL unsupported", "netstackd: net fail-code 0x0001")
                        }
                        nexus_net::NetError::NoBufs => {
                            ("netstackd: net FAIL no-bufs", "netstackd: net fail-code 0x0002")
                        }
                        nexus_net::NetError::Internal(msg) => match msg {
                            "mmio cap not found" => (
                                "netstackd: net FAIL mmio-cap-missing",
                                "netstackd: net fail-code 0x0101",
                            ),
                            "mmio_map failed" => {
                                ("netstackd: net FAIL mmio-map", "netstackd: net fail-code 0x0102")
                            }
                            "virtio probe failed" => (
                                "netstackd: net FAIL virtio-probe",
                                "netstackd: net fail-code 0x0103",
                            ),
                            "virtio features" => (
                                "netstackd: net FAIL virtio-features",
                                "netstackd: net fail-code 0x0104",
                            ),
                            _ => {
                                ("netstackd: net FAIL internal", "netstackd: net fail-code 0x01ff")
                            }
                        },
                        _ => ("netstackd: net FAIL other", "netstackd: net fail-code 0x00ff"),
                    };
                    let _ = nexus_abi::debug_println(fail_label);
                    let _ = nexus_abi::debug_println(fail_diag_code);
                    let _ = nexus_abi::debug_println("netstackd: halt net-init-fail");
                    loop {
                        let _ = yield_();
                    }
                }
            }
        }
    };

    let _ = nexus_abi::debug_println("net: virtio-net up");
    let _ = nexus_abi::debug_println("SELFTEST: net iface ok");

    let mut dhcp_bound = false;
    let mut dhcp_ms: u64 = 0;
    let dhcp_deadline_ms: u64 = if cfg!(feature = "qemu-smoke") { 30_000 } else { 4_000 };
    loop {
        net.poll(dhcp_ms);
        if let Some(config) = net.dhcp_poll() {
            crate::os::observability::emit_dhcp_bound_marker(&config);
            dhcp_bound = true;
            break;
        }
        if dhcp_ms >= dhcp_deadline_ms {
            break;
        }
        dhcp_ms = dhcp_ms.saturating_add(1);
        let _ = yield_();
    }
    if !dhcp_bound {
        let _ = nexus_abi::debug_println("net: dhcp timeout (fallback static)");
        let is_qemu_smoke = cfg!(feature = "qemu-smoke");
        let mac = net.mac();
        let (ip, prefix_len, gw) = crate::os::config::fallback_ipv4_config(is_qemu_smoke, mac);
        if is_qemu_smoke {
            let _ = nexus_abi::debug_println("dbg:netstackd: fallback profile qemu-smoke");
        } else {
            let _ = nexus_abi::debug_println("dbg:netstackd: fallback profile os2vm-static");
        }
        net.set_static_ipv4(ip, prefix_len, gw);
        crate::os::observability::emit_fallback_static_marker(ip, prefix_len);
    }

    if let Some(config) = net.get_ipv4_config().or_else(|| net.get_dhcp_config()) {
        crate::os::observability::emit_smoltcp_iface_marker(&config);
    }

    if dhcp_bound {
        let ping_start = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        if net.probe_ping_gateway(ping_start, 4000).is_err() {
            let _ = nexus_abi::debug_println("netstackd: net ping FAIL");
            let _ = nexus_abi::debug_println("netstackd: halt net-ping-fail");
            loop {
                let _ = yield_();
            }
        }
        let _ = nexus_abi::debug_println("SELFTEST: net ping ok");
    }

    for _ in 0..4096u64 {
        let _ = yield_();
    }

    if dhcp_bound {
        let dns_a = NetSocketAddrV4::new(crate::os::entry_pure::QEMU_USERNET_DNS_PRIMARY_IP, 53);
        let dns_b = NetSocketAddrV4::new(crate::os::entry_pure::QEMU_USERNET_GATEWAY_IP, 53);
        let ipcfg = net.get_ipv4_config();
        let bind_ip =
            ipcfg.map(|c| c.ip).unwrap_or(crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP);
        // #region agent log (H4: ensure IP config exists at DNS proof start)
        if ipcfg.is_none() {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h4 no-ipcfg");
        }
        let _ = nexus_abi::debug_println("dbg:netstackd: dns h-start");
        // #endregion
        let mut sock = match net.udp_bind(NetSocketAddrV4::new(bind_ip, 40_000)) {
            Ok(s) => s,
            Err(_) => {
                let _ = nexus_abi::debug_println("netstackd: udp bind FAIL");
                let _ = nexus_abi::debug_println("netstackd: halt net-udp-bind-fail");
                loop {
                    let _ = yield_();
                }
            }
        };

        let mut q = [0u8; 32];
        q[0] = 0x12;
        q[1] = 0x34;
        q[2] = 0x01;
        q[3] = 0x00;
        q[4] = 0x00;
        q[5] = 0x01;
        let mut p = 12usize;
        q[p] = 9;
        p += 1;
        q[p..p + 9].copy_from_slice(b"localhost");
        p += 9;
        q[p] = 0;
        p += 1;
        q[p] = 0;
        q[p + 1] = 1;
        q[p + 2] = 0;
        q[p + 3] = 1;
        p += 4;

        let mut ok = false;
        let mut logged_diag = false;
        let mut buf = [0u8; 512];
        let mut any_send_attempt = false;
        let mut saw_send_ok = false;
        let mut saw_send_would_block = false;
        let mut saw_send_no_bufs = false;
        let mut saw_send_other = false;
        let mut saw_recv_packet = false;
        let mut saw_recv_would_block = false;
        let mut saw_recv_other = false;
        let mut deadline_hit = false;
        for _ in 0..256u64 {
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            net.poll(now_ms);
            let _ = yield_();
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let dns_budget_ms = if cfg!(feature = "qemu-smoke") { 1_500 } else { 500 };
        let deadline_ms = start_ms.saturating_add(dns_budget_ms);
        let mut last_send_ms = 0u64;
        for i in 0..200_000u64 {
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            net.poll(now_ms);
            if now_ms >= deadline_ms {
                deadline_hit = true;
                break;
            }
            if i == 0 || now_ms.saturating_sub(last_send_ms) >= 100 {
                any_send_attempt = true;
                match sock.send_to(&q[..p], dns_a) {
                    Ok(_) => saw_send_ok = true,
                    Err(NetError::WouldBlock) => saw_send_would_block = true,
                    Err(NetError::NoBufs) => saw_send_no_bufs = true,
                    Err(_) => saw_send_other = true,
                }
                match sock.send_to(&q[..p], dns_b) {
                    Ok(_) => saw_send_ok = true,
                    Err(NetError::WouldBlock) => saw_send_would_block = true,
                    Err(NetError::NoBufs) => saw_send_no_bufs = true,
                    Err(_) => saw_send_other = true,
                }
                last_send_ms = now_ms;
            }
            for _ in 0..4 {
                match sock.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        saw_recv_packet = true;
                        if crate::os::entry_pure::is_dns_probe_response(&buf[..n], from.port) {
                            ok = true;
                            break;
                        }
                        if !logged_diag {
                            logged_diag = true;
                            // #region agent log (H3: classify first unexpected DNS UDP packet)
                            let mut msg = [0u8; 64];
                            let mut pos = 0usize;
                            let prefix = b"dbg:netstackd: dns h3 rx-other-from ";
                            msg[pos..pos + prefix.len()].copy_from_slice(prefix);
                            pos += prefix.len();
                            pos += crate::os::observability::write_ip(&from.ip.0, &mut msg[pos..]);
                            if let Ok(s) = core::str::from_utf8(&msg[..pos]) {
                                let _ = nexus_abi::debug_println(s);
                            }
                            if from.ip.0 != crate::os::entry_pure::QEMU_USERNET_DNS_PRIMARY_IP
                                && from.ip.0 != crate::os::entry_pure::QEMU_USERNET_GATEWAY_IP
                            {
                                let _ =
                                    nexus_abi::debug_println("dbg:netstackd: dns h3 rx-other-src");
                            } else if from.port != 53 {
                                let _ =
                                    nexus_abi::debug_println("dbg:netstackd: dns h3 rx-other-port");
                            } else if n < 2 {
                                let _ = nexus_abi::debug_println(
                                    "dbg:netstackd: dns h3 rx-other-short",
                                );
                            } else if buf[0] != 0x12 || buf[1] != 0x34 {
                                let _ =
                                    nexus_abi::debug_println("dbg:netstackd: dns h3 rx-other-xid");
                            } else {
                                let _ = nexus_abi::debug_println(
                                    "dbg:netstackd: dns h3 rx-other-unknown",
                                );
                            }
                            // #endregion
                            let _ = nexus_abi::debug_println("netstackd: udp dns rx other");
                        }
                    }
                    Err(NetError::WouldBlock) => {
                        saw_recv_would_block = true;
                        break;
                    }
                    Err(_) => {
                        saw_recv_other = true;
                        break;
                    }
                }
            }
            if ok {
                break;
            }
            let _ = yield_();
        }
        // #region agent log (H1/H2/H3 summary of DNS probe path)
        if !any_send_attempt {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h1 no-send");
        }
        if saw_send_ok {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h2 send-ok");
        }
        if saw_send_would_block {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h2 send-wouldblock");
        }
        if saw_send_no_bufs {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h2 send-nobufs");
        }
        if saw_send_other {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h2 send-other");
        }
        if !saw_recv_packet && saw_recv_would_block {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h3 recv-wouldblock-only");
        }
        if saw_recv_other {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h3 recv-other");
        }
        if deadline_hit {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns h1 deadline");
        }
        if ok {
            let _ = nexus_abi::debug_println("dbg:netstackd: dns matched");
        }
        // #endregion
        if !ok {
            let _ =
                nexus_abi::debug_println("netstackd: udp dns unavailable (fallback dhcp proof)");
            let _ = nexus_abi::debug_println("netstackd: net dns proof fail");
        } else {
            let _ = nexus_abi::debug_println("SELFTEST: net udp dns ok");
        }
    }

    let bind_ip = net
        .get_ipv4_config()
        .map(|c| c.ip)
        .unwrap_or(crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP);
    if net.tcp_listen(NetSocketAddrV4::new(bind_ip, 41_000), 1).is_ok() {
        let _ = nexus_abi::debug_println("SELFTEST: net tcp listen ok");
    } else {
        let _ = nexus_abi::debug_println("netstackd: tcp listen FAIL");
        let _ = nexus_abi::debug_println("netstackd: halt net-tcp-listen-fail");
        loop {
            let _ = yield_();
        }
    }
    let _ = nexus_abi::debug_println("netstackd: facade up");

    BootstrapResult { net, bind_ip }
}
