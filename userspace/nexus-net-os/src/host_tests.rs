//! Host-first deterministic tests for OS networking integration logic.
//!
//! NOTE: These tests intentionally avoid QEMU and MMIO. They validate pure logic that should
//! remain stable even when virtio backends or QEMU versions change.
//!
//! IMPORTANT: We intentionally do **not** depend on smoltcp's internal DHCP wire/config structs
//! here. smoltcp's DHCP config types include borrowed packet references and other details that
//! are not relevant to our OS integration logic and tend to shift across versions. Our goal is:
//! - deterministically detect "new lease vs same lease"
//! - track configured vs deconfigured
//! - produce a stable marker summary type

/// Minimal DHCP integration state machine:
/// - Detect "new" vs "same" config
/// - Track current lease
/// - Provide a stable `DhcpConfig` summary for marker emission
///
/// This is a host-testable mirror of the behavior used by `SmoltcpVirtioNetStack::dhcp_poll()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DhcpState {
    bound_ip: Option<([u8; 4], u8)>,
    bound_gateway: Option<[u8; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DhcpConfig {
    ip: [u8; 4],
    prefix_len: u8,
    gateway: Option<[u8; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DhcpUpdate {
    Configured(DhcpConfig),
    Deconfigured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Configured { ip: [u8; 4], prefix_len: u8, gateway: Option<[u8; 4]> },
    Deconfigured,
}

// -----------------------------------------------------------------------------
// On-wire invariants (host-first)
// -----------------------------------------------------------------------------

#[test]
fn dhcp_discover_uses_standard_ports_and_broadcast() {
    use smoltcp::iface::{Config as IfaceConfig, Interface, SocketSet};
    use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
    use smoltcp::socket::dhcpv4;
    use smoltcp::time::Instant;
    use smoltcp::wire::{
        EthernetAddress, EthernetFrame, HardwareAddress, IpProtocol, Ipv4Address, Ipv4Packet,
        UdpPacket,
    };

    struct CaptureDevice {
        caps: DeviceCapabilities,
        tx: std::vec::Vec<std::vec::Vec<u8>>,
    }

    struct CaptTx<'a> {
        dev: &'a mut CaptureDevice,
    }

    impl TxToken for CaptTx<'_> {
        fn consume<R, F>(self, len: usize, f: F) -> R
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            let mut buf = vec![0u8; len];
            let r = f(&mut buf);
            self.dev.tx.push(buf);
            r
        }
    }

    struct NoRx;
    impl RxToken for NoRx {
        fn consume<R, F>(self, _f: F) -> R
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            unreachable!("no RX in this test")
        }
    }

    impl Device for CaptureDevice {
        type RxToken<'a>
            = NoRx
        where
            Self: 'a;
        type TxToken<'a>
            = CaptTx<'a>
        where
            Self: 'a;

        fn capabilities(&self) -> DeviceCapabilities {
            self.caps.clone()
        }

        fn receive(
            &mut self,
            _timestamp: Instant,
        ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
            None
        }

        fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
            Some(CaptTx { dev: self })
        }
    }

    let mut dev = CaptureDevice {
        caps: {
            let mut c = DeviceCapabilities::default();
            c.medium = Medium::Ethernet;
            c.max_transmission_unit = 1500;
            c
        },
        tx: std::vec::Vec::new(),
    };

    let mac = EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let hw = HardwareAddress::Ethernet(mac);
    let mut cfg = IfaceConfig::new(hw);
    cfg.random_seed = 0x1234_5678;
    let mut iface = Interface::new(cfg, &mut dev, Instant::from_millis(0));

    let mut sockets = SocketSet::new(std::vec::Vec::new());
    let dhcp = dhcpv4::Socket::new();
    let dh = sockets.add(dhcp);

    // Drive the DHCP socket similarly to the OS loop:
    // - poll interface
    // - poll dhcp socket (state machine)
    // - poll interface again to flush outgoing packets
    for t in 0..20u64 {
        let now = Instant::from_millis((t * 50) as i64);
        let _ = iface.poll(now, &mut dev, &mut sockets);
        let _ = sockets.get_mut::<dhcpv4::Socket>(dh).poll();
        let _ = iface.poll(now, &mut dev, &mut sockets);
    }

    // Assert at least one outbound UDP frame uses DHCP client/server ports.
    let mut saw = false;
    for frame in &dev.tx {
        let Ok(eth) = EthernetFrame::new_checked(frame) else {
            continue;
        };
        if eth.ethertype() != smoltcp::wire::EthernetProtocol::Ipv4 {
            continue;
        }
        let Ok(ip) = Ipv4Packet::new_checked(eth.payload()) else {
            continue;
        };
        if ip.next_header() != IpProtocol::Udp {
            continue;
        }
        let Ok(udp) = UdpPacket::new_checked(ip.payload()) else {
            continue;
        };
        let src = udp.src_port();
        let dst = udp.dst_port();
        if src == 68 && dst == 67 {
            saw = true;
            // RFC 2131 discover/request originate from 0.0.0.0 and broadcast to 255.255.255.255.
            let s = ip.src_addr();
            let d = ip.dst_addr();
            assert_eq!(s, Ipv4Address::new(0, 0, 0, 0));
            assert_eq!(d, Ipv4Address::new(255, 255, 255, 255));
            break;
        }
    }
    assert!(saw, "no DHCP UDP frame (68->67) captured");
}

impl DhcpState {
    fn handle_event(&mut self, ev: Event) -> Option<DhcpUpdate> {
        match ev {
            Event::Configured { ip, prefix_len, gateway } => {
                let is_new =
                    self.bound_ip != Some((ip, prefix_len)) || self.bound_gateway != gateway;
                if !is_new {
                    return None;
                }
                self.bound_ip = Some((ip, prefix_len));
                self.bound_gateway = gateway;
                Some(DhcpUpdate::Configured(DhcpConfig { ip, prefix_len, gateway }))
            }
            Event::Deconfigured => {
                self.bound_ip = None;
                self.bound_gateway = None;
                Some(DhcpUpdate::Deconfigured)
            }
        }
    }
}

fn mk_cfg(ip: [u8; 4], prefix_len: u8, gateway: Option<[u8; 4]>) -> Event {
    Event::Configured { ip, prefix_len, gateway }
}

#[test]
fn dhcp_configured_first_time_emits_update() {
    let mut st = DhcpState::default();
    let ev = mk_cfg([10, 0, 2, 15], 24, Some([10, 0, 2, 2]));
    let upd = st.handle_event(ev);
    assert_eq!(
        upd,
        Some(DhcpUpdate::Configured(DhcpConfig {
            ip: [10, 0, 2, 15],
            prefix_len: 24,
            gateway: Some([10, 0, 2, 2]),
        }))
    );
}

#[test]
fn dhcp_same_config_is_silent() {
    let mut st = DhcpState::default();
    let ev = mk_cfg([10, 0, 2, 15], 24, Some([10, 0, 2, 2]));
    assert!(matches!(st.handle_event(ev), Some(DhcpUpdate::Configured(_))));
    // Same again => None
    let ev2 = mk_cfg([10, 0, 2, 15], 24, Some([10, 0, 2, 2]));
    assert_eq!(st.handle_event(ev2), None);
}

#[test]
fn dhcp_gateway_change_emits_update() {
    let mut st = DhcpState::default();
    let ev = mk_cfg([10, 0, 2, 15], 24, Some([10, 0, 2, 2]));
    let _ = st.handle_event(ev);
    // Gateway changed => update
    let ev2 = mk_cfg([10, 0, 2, 15], 24, Some([10, 0, 2, 254]));
    assert_eq!(
        st.handle_event(ev2),
        Some(DhcpUpdate::Configured(DhcpConfig {
            ip: [10, 0, 2, 15],
            prefix_len: 24,
            gateway: Some([10, 0, 2, 254]),
        }))
    );
}

#[test]
fn dhcp_deconfigured_clears_lease() {
    let mut st = DhcpState::default();
    let ev = mk_cfg([10, 0, 2, 15], 24, Some([10, 0, 2, 2]));
    let _ = st.handle_event(ev);
    assert_eq!(st.bound_ip.map(|(ip, _p)| ip), Some([10, 0, 2, 15]));
    assert_eq!(st.handle_event(Event::Deconfigured), Some(DhcpUpdate::Deconfigured));
    assert_eq!(st.bound_ip, None);
    assert_eq!(st.bound_gateway, None);
}
