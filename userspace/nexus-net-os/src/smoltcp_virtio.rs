//! CONTEXT: smoltcp + virtio-net backend implementing the `nexus-net` facade.
//! SAFETY: This module contains narrowly-scoped unsafe MMIO/DMA logic.

extern crate alloc;

use alloc::rc::Rc;
use core::cell::RefCell;

use nexus_net::{
    validate_tcp_write_len, validate_udp_payload_len, NetError, NetInstant, NetSocketAddrV4,
    NetStack, TcpListener, TcpStream, UdpSocket,
};

use net_virtio::{QueueSetup, VirtioNetMmio, VIRTIO_DEVICE_ID_NET, VIRTIO_MMIO_MAGIC};
use nexus_abi::{cap_query, mmio_map, vmo_create, vmo_map_page_sys, AbiError, CapQuery};
use nexus_hal::Bus;
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::dhcpv4::{self, Event as DhcpEvent};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr, Ipv4Address,
};

const VIRTQ_DESC_F_WRITE: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct VqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VqAvail<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [u16; N],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VqUsed<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [VqUsedElem; N],
}

struct MmioBus {
    base: usize,
}

impl Bus for MmioBus {
    fn read(&self, addr: usize) -> u32 {
        // SAFETY: MMIO mapped region; volatile read required.
        unsafe { core::ptr::read_volatile((self.base + addr) as *const u32) }
    }
    fn write(&self, addr: usize, value: u32) {
        // SAFETY: MMIO mapped region; volatile write required.
        unsafe { core::ptr::write_volatile((self.base + addr) as *mut u32, value) }
    }
}

fn align4(x: usize) -> usize {
    (x + 3) & !3usize
}

fn mmio_map_ok(mmio_cap_slot: u32, va: usize, off: usize) -> Result<(), NetError> {
    match mmio_map(mmio_cap_slot, va, off) {
        Ok(()) => Ok(()),
        Err(AbiError::InvalidArgument) => Ok(()), // already mapped
        Err(_) => Err(NetError::Internal("mmio_map failed")),
    }
}

fn cap_query_base_len(slot: u32) -> Result<(u64, u64), NetError> {
    let mut info = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    cap_query(slot, &mut info).map_err(|_| NetError::Internal("cap_query failed"))?;
    Ok((info.base, info.len))
}

pub struct SmoltcpVirtioNetStack {
    inner: Rc<RefCell<Inner>>,
}

// Bring-up sizing knobs: kept small to reduce page-table pressure and kernel heap usage.
const Q_LEN: usize = 8;
const ACTIVE_BUFS: usize = 1;

struct Inner {
    // Virtio device (MMIO)
    dev: VirtioNetMmio<MmioBus>,

    // Queue state (fixed to legacy mmio layout for QEMU virt today).
    rx_desc: *mut VqDesc,
    rx_avail: *mut VqAvail<Q_LEN>,
    rx_used: *mut VqUsed<Q_LEN>,
    rx_last_used: u16,
    tx_desc: *mut VqDesc,
    tx_avail: *mut VqAvail<Q_LEN>,
    tx_used: *mut VqUsed<Q_LEN>,
    tx_last_used: u16,

    rx_buf_va: [usize; ACTIVE_BUFS],
    rx_buf_pa: [u64; ACTIVE_BUFS],
    tx_buf_va: [usize; ACTIVE_BUFS],
    tx_buf_pa: [u64; ACTIVE_BUFS],
    tx_free: [bool; ACTIVE_BUFS],

    iface: Interface,
    sockets: SocketSet<'static>,

    // DHCP client socket handle
    dhcp_handle: SocketHandle,

    // DHCP state: bound IP (None until lease acquired)
    dhcp_bound_ip: Option<smoltcp::wire::Ipv4Cidr>,
    dhcp_bound_gateway: Option<Ipv4Address>,

    // Monotonic tick (backend-defined units; we use ms).
    now: NetInstant,
}

impl SmoltcpVirtioNetStack {
    /// Bring up virtio-net + smoltcp using the standard selftest device injection:
    /// - Device MMIO cap in slot 48
    /// - virtio-mmio base mapped at 0x2000_e000 and scanned for a net device.
    pub fn new_default() -> Result<Self, NetError> {
        const MMIO_CAP_SLOT: u32 = 48;
        const MMIO_VA: usize = 0x2000_e000;
        const SLOT_STRIDE: usize = 0x1000;
        const MAX_SLOTS: usize = 8;

        mmio_map_ok(MMIO_CAP_SLOT, MMIO_VA, 0)?;

        let mut found: Option<usize> = None;
        for slot in 0..MAX_SLOTS {
            let off = slot * SLOT_STRIDE;
            let va = MMIO_VA + off;
            if slot != 0 {
                if mmio_map_ok(MMIO_CAP_SLOT, va, off).is_err() {
                    continue;
                }
            }
            // VirtIO MMIO registers: magic @ 0x000, device_id @ 0x008
            // SAFETY: MMIO is mapped by mmio_map_ok.
            let magic = unsafe { core::ptr::read_volatile((va + 0x000) as *const u32) };
            if magic != VIRTIO_MMIO_MAGIC {
                continue;
            }
            let device_id = unsafe { core::ptr::read_volatile((va + 0x008) as *const u32) };
            if device_id == VIRTIO_DEVICE_ID_NET {
                found = Some(slot);
                break;
            }
        }
        let slot = found.ok_or(NetError::Unsupported)?;
        let dev_va = MMIO_VA + slot * SLOT_STRIDE;
        let dev = VirtioNetMmio::new(MmioBus { base: dev_va });
        dev.probe().map_err(|_| NetError::Internal("virtio probe failed"))?;

        // Negotiate a minimal feature set: accept MAC in config space.
        const VIRTIO_NET_F_MAC: u64 = 1 << 5;
        dev.reset();
        dev.negotiate_features(VIRTIO_NET_F_MAC)
            .map_err(|_| NetError::Internal("virtio features"))?;

        // Queue memory (2 pages): 1 page per queue, small bring-up queues.
        const Q_MEM_VA: usize = 0x2002_0000;
        const Q_PAGES: usize = 2;
        let q_vmo = vmo_create(Q_PAGES * 4096).map_err(|_| NetError::NoBufs)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for page in 0..Q_PAGES {
            let va = Q_MEM_VA + page * 4096;
            let off = page * 4096;
            vmo_map_page_sys(q_vmo, va, off, flags).map_err(|_| NetError::NoBufs)?;
        }
        let (q_base_pa, _q_len) = cap_query_base_len(q_vmo as u32)?;

        // Legacy combined layout within each queue:
        // desc[Q_LEN] + avail + used (aligned to 4), all within 2 pages.
        let q_len = Q_LEN;
        let desc_bytes = core::mem::size_of::<VqDesc>() * q_len;
        let avail_bytes = core::mem::size_of::<VqAvail<Q_LEN>>();
        let used_off = align4(desc_bytes + avail_bytes);

        let q0_va = Q_MEM_VA + 0;
        let q1_va = Q_MEM_VA + 4096;

        let rx_desc_va = q0_va;
        let rx_avail_va = q0_va + desc_bytes;
        let rx_used_va = q0_va + used_off;

        let tx_desc_va = q1_va;
        let tx_avail_va = q1_va + desc_bytes;
        let tx_used_va = q1_va + used_off;

        // For legacy, setup_queue uses desc_paddr PFN; other paddr fields are ignored.
        dev.setup_queue(
            0,
            &QueueSetup {
                size: q_len as u16,
                desc_paddr: q_base_pa + 0,
                avail_paddr: 0,
                used_paddr: 0,
            },
        )
        .map_err(|_| NetError::Internal("setup q0"))?;
        dev.setup_queue(
            1,
            &QueueSetup {
                size: q_len as u16,
                desc_paddr: q_base_pa + 4096,
                avail_paddr: 0,
                used_paddr: 0,
            },
        )
        .map_err(|_| NetError::Internal("setup q1"))?;

        // Buffers: ACTIVE_BUFS RX pages + ACTIVE_BUFS TX pages.
        const BUF_VA: usize = 0x2004_0000;
        let buf_vmo = vmo_create(ACTIVE_BUFS * 2 * 4096).map_err(|_| NetError::NoBufs)?;
        for page in 0..(ACTIVE_BUFS * 2) {
            let va = BUF_VA + page * 4096;
            let off = page * 4096;
            vmo_map_page_sys(buf_vmo, va, off, flags).map_err(|_| NetError::NoBufs)?;
        }
        let (buf_base_pa, _buf_len) = cap_query_base_len(buf_vmo as u32)?;

        // Zero queue pages.
        // SAFETY: mapped Q_MEM_VA points to the VMO mapping for queue memory.
        unsafe { core::ptr::write_bytes(Q_MEM_VA as *mut u8, 0, Q_PAGES * 4096) };

        let mut rx_buf_va = [0usize; ACTIVE_BUFS];
        let mut rx_buf_pa = [0u64; ACTIVE_BUFS];
        let mut tx_buf_va = [0usize; ACTIVE_BUFS];
        let mut tx_buf_pa = [0u64; ACTIVE_BUFS];
        let tx_free = [true; ACTIVE_BUFS];
        for i in 0..ACTIVE_BUFS {
            rx_buf_va[i] = BUF_VA + i * 4096;
            rx_buf_pa[i] = buf_base_pa + (i as u64) * 4096;
            tx_buf_va[i] = BUF_VA + (ACTIVE_BUFS + i) * 4096;
            tx_buf_pa[i] = buf_base_pa + ((ACTIVE_BUFS + i) as u64) * 4096;
        }

        // Read MAC from config space (0x100).
        let mac = {
            let w0 = unsafe { core::ptr::read_volatile((dev_va + 0x100) as *const u32) };
            let w1 = unsafe { core::ptr::read_volatile((dev_va + 0x104) as *const u32) };
            [
                (w0 & 0xff) as u8,
                ((w0 >> 8) & 0xff) as u8,
                ((w0 >> 16) & 0xff) as u8,
                ((w0 >> 24) & 0xff) as u8,
                (w1 & 0xff) as u8,
                ((w1 >> 8) & 0xff) as u8,
            ]
        };

        // smoltcp setup
        let hw = HardwareAddress::Ethernet(EthernetAddress(mac));
        let mut cfg = IfaceConfig::new(hw);
        cfg.random_seed = 0x1234_5678;

        // Create a socket set with owned storage.
        let mut sockets = SocketSet::new(alloc::vec::Vec::new());

        // Create DHCPv4 socket for automatic IP configuration.
        let dhcp_socket = dhcpv4::Socket::new();
        let dhcp_handle = sockets.add(dhcp_socket);

        // Temporary device wrapper for iface init.
        let mut devwrap: SmolDevice<ACTIVE_BUFS> = SmolDevice {
            dev: &dev as *const _,
            rx_desc: rx_desc_va as *mut VqDesc,
            rx_avail: rx_avail_va as *mut VqAvail<Q_LEN>,
            rx_used: rx_used_va as *mut VqUsed<Q_LEN>,
            rx_last_used: 0,
            tx_desc: tx_desc_va as *mut VqDesc,
            tx_avail: tx_avail_va as *mut VqAvail<Q_LEN>,
            tx_used: tx_used_va as *mut VqUsed<Q_LEN>,
            tx_last_used: 0,
            rx_buf_va,
            rx_buf_pa,
            tx_buf_va,
            tx_buf_pa,
            tx_free,
        };
        devwrap.rx_post(ACTIVE_BUFS);
        dev.set_driver_ok();

        // Initialize interface WITHOUT static IP — DHCP will configure it.
        let iface = Interface::new(cfg, &mut devwrap, Instant::from_millis(0));
        // NOTE: No static IP or route added here; DHCP will provide them.

        Ok(Self {
            inner: Rc::new(RefCell::new(Inner {
                dev,
                rx_desc: devwrap.rx_desc,
                rx_avail: devwrap.rx_avail,
                rx_used: devwrap.rx_used,
                rx_last_used: devwrap.rx_last_used,
                tx_desc: devwrap.tx_desc,
                tx_avail: devwrap.tx_avail,
                tx_used: devwrap.tx_used,
                tx_last_used: devwrap.tx_last_used,
                rx_buf_va: devwrap.rx_buf_va,
                rx_buf_pa: devwrap.rx_buf_pa,
                tx_buf_va: devwrap.tx_buf_va,
                tx_buf_pa: devwrap.tx_buf_pa,
                tx_free: devwrap.tx_free,
                iface,
                sockets,
                dhcp_handle,
                dhcp_bound_ip: None,
                dhcp_bound_gateway: None,
                now: 0,
            })),
        })
    }

    /// Best-effort ICMP echo probe to the QEMU usernet gateway (10.0.2.2).
    ///
    /// This is intentionally a bounded proof hook and not part of the public sockets facade.
    pub fn probe_ping_gateway(
        &mut self,
        start_ms: NetInstant,
        max_polls: usize,
    ) -> Result<(), NetError> {
        let mut inner = self.inner.borrow_mut();
        let rx_meta = [smoltcp::socket::icmp::PacketMetadata::EMPTY; 4];
        let tx_meta = [smoltcp::socket::icmp::PacketMetadata::EMPTY; 4];
        let rx_buf = smoltcp::socket::icmp::PacketBuffer::new(rx_meta, alloc::vec![0u8; 256]);
        let tx_buf = smoltcp::socket::icmp::PacketBuffer::new(tx_meta, alloc::vec![0u8; 256]);
        let mut icmp = smoltcp::socket::icmp::Socket::new(rx_buf, tx_buf);
        icmp.bind(smoltcp::socket::icmp::Endpoint::Ident(0x1234))
            .map_err(|_| NetError::InvalidInput("icmp bind"))?;
        let handle = inner.sockets.add(icmp);
        drop(inner);

        let target = IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 2));
        let checksum = smoltcp::phy::ChecksumCapabilities::default();
        let mut sent = false;
        for i in 0..max_polls {
            let now = start_ms.saturating_add(i as u64);
            self.poll(now);
            {
                let mut inner = self.inner.borrow_mut();
                let sock = inner.sockets.get_mut::<smoltcp::socket::icmp::Socket>(handle);
                if !sent && sock.can_send() {
                    let mut bytes = [0u8; 24];
                    let mut pkt = Icmpv4Packet::new_unchecked(&mut bytes);
                    let repr =
                        Icmpv4Repr::EchoRequest { ident: 0x1234, seq_no: 1, data: &[0u8; 16] };
                    repr.emit(&mut pkt, &checksum);
                    let _ = sock.send_slice(pkt.into_inner(), target);
                    sent = true;
                }
                if sock.can_recv() {
                    let _ = sock.recv();
                    inner.sockets.remove(handle);
                    return Ok(());
                }
            }
        }
        // Keep socket allocated (best-effort); caller will drop the stack anyway.
        Err(NetError::TimedOut)
    }

    /// Poll the DHCP client and handle configuration events.
    ///
    /// Returns `Some(DhcpConfig)` when a new lease is acquired or reconfigured.
    /// Caller should emit the marker `net: dhcp bound <ip>/<prefix> gw=<gw>` on first acquisition.
    ///
    /// Note: smoltcp 0.10 handles neighbor cache maintenance automatically when the IP changes.
    pub fn dhcp_poll(&mut self) -> Option<DhcpConfig> {
        let mut inner = self.inner.borrow_mut();
        let dhcp_handle = inner.dhcp_handle;

        // Poll DHCP socket for events
        let event = inner.sockets.get_mut::<dhcpv4::Socket>(dhcp_handle).poll();

        match event {
            Some(DhcpEvent::Configured(config)) => {
                let address = config.address;
                let router = config.router;

                // Check if this is a new/changed configuration
                let is_new_config = inner.dhcp_bound_ip != Some(address)
                    || inner.dhcp_bound_gateway != router;

                if is_new_config {
                    // Update interface IP addresses
                    inner.iface.update_ip_addrs(|addrs| {
                        addrs.clear();
                        let _ = addrs.push(IpCidr::new(
                            IpAddress::Ipv4(address.address()),
                            address.prefix_len(),
                        ));
                    });

                    // Update routes
                    inner.iface.routes_mut().remove_default_ipv4_route();
                    if let Some(gw) = router {
                        let _ = inner.iface.routes_mut().add_default_ipv4_route(gw);
                    }

                    // Store bound configuration
                    inner.dhcp_bound_ip = Some(address);
                    inner.dhcp_bound_gateway = router;

                    Some(DhcpConfig {
                        ip: address.address().0,
                        prefix_len: address.prefix_len(),
                        gateway: router.map(|gw| gw.0),
                    })
                } else {
                    None
                }
            }
            Some(DhcpEvent::Deconfigured) => {
                // Lease expired or lost
                inner.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                });
                inner.iface.routes_mut().remove_default_ipv4_route();
                inner.dhcp_bound_ip = None;
                inner.dhcp_bound_gateway = None;
                None
            }
            None => None,
        }
    }

    /// Check if DHCP lease has been acquired.
    pub fn is_dhcp_bound(&self) -> bool {
        self.inner.borrow().dhcp_bound_ip.is_some()
    }

    /// Get the currently bound DHCP configuration, if any.
    pub fn get_dhcp_config(&self) -> Option<DhcpConfig> {
        let inner = self.inner.borrow();
        let address = inner.dhcp_bound_ip?;
        Some(DhcpConfig {
            ip: address.address().0,
            prefix_len: address.prefix_len(),
            gateway: inner.dhcp_bound_gateway.map(|gw| gw.0),
        })
    }

    /// ICMP ping to a target address with cooperative yielding.
    ///
    /// Returns Ok(rtt_ms) on success (round-trip time in milliseconds).
    /// This is a bounded, non-blocking helper for QEMU proof markers.
    pub fn icmp_ping(
        &mut self,
        target_ip: [u8; 4],
        start_ms: NetInstant,
        timeout_ms: u64,
    ) -> Result<u64, NetError> {
        let mut inner = self.inner.borrow_mut();
        let rx_meta = [smoltcp::socket::icmp::PacketMetadata::EMPTY; 4];
        let tx_meta = [smoltcp::socket::icmp::PacketMetadata::EMPTY; 4];
        let rx_buf = smoltcp::socket::icmp::PacketBuffer::new(rx_meta, alloc::vec![0u8; 256]);
        let tx_buf = smoltcp::socket::icmp::PacketBuffer::new(tx_meta, alloc::vec![0u8; 256]);
        let mut icmp = smoltcp::socket::icmp::Socket::new(rx_buf, tx_buf);
        // Use a deterministic ident for bring-up
        let ident: u16 = 0x4E58; // "NX"
        icmp.bind(smoltcp::socket::icmp::Endpoint::Ident(ident))
            .map_err(|_| NetError::InvalidInput("icmp bind"))?;
        let handle = inner.sockets.add(icmp);
        drop(inner);

        let target = IpAddress::Ipv4(Ipv4Address::from_bytes(&target_ip));
        let checksum = smoltcp::phy::ChecksumCapabilities::default();
        let mut sent = false;
        let mut send_time: u64 = 0;

        let max_polls = timeout_ms as usize;
        for i in 0..max_polls {
            let now = start_ms.saturating_add(i as u64);
            self.poll(now);
            {
                let mut inner = self.inner.borrow_mut();
                let sock = inner.sockets.get_mut::<smoltcp::socket::icmp::Socket>(handle);
                if !sent && sock.can_send() {
                    let mut bytes = [0u8; 24];
                    let mut pkt = Icmpv4Packet::new_unchecked(&mut bytes);
                    let repr = Icmpv4Repr::EchoRequest { ident, seq_no: 1, data: &[0u8; 16] };
                    repr.emit(&mut pkt, &checksum);
                    let _ = sock.send_slice(pkt.into_inner(), target);
                    sent = true;
                    send_time = now;
                }
                if sock.can_recv() {
                    let _ = sock.recv();
                    inner.sockets.remove(handle);
                    let rtt = now.saturating_sub(send_time);
                    return Ok(rtt);
                }
            }
        }
        let mut inner = self.inner.borrow_mut();
        inner.sockets.remove(handle);
        Err(NetError::TimedOut)
    }
}

/// DHCP configuration result for marker output.
#[derive(Clone, Copy, Debug)]
pub struct DhcpConfig {
    /// IPv4 address bytes.
    pub ip: [u8; 4],
    /// Prefix length (e.g., 24 for /24).
    pub prefix_len: u8,
    /// Gateway address bytes, if provided.
    pub gateway: Option<[u8; 4]>,
}

// smoltcp Device wrapper around our virtqueue implementation.
struct SmolDevice<const ACTIVE: usize = 16> {
    dev: *const VirtioNetMmio<MmioBus>,
    rx_desc: *mut VqDesc,
    rx_avail: *mut VqAvail<Q_LEN>,
    rx_used: *mut VqUsed<Q_LEN>,
    rx_last_used: u16,
    tx_desc: *mut VqDesc,
    tx_avail: *mut VqAvail<Q_LEN>,
    tx_used: *mut VqUsed<Q_LEN>,
    tx_last_used: u16,
    rx_buf_va: [usize; ACTIVE],
    rx_buf_pa: [u64; ACTIVE],
    tx_buf_va: [usize; ACTIVE],
    tx_buf_pa: [u64; ACTIVE],
    tx_free: [bool; ACTIVE],
}

impl<const ACTIVE: usize> SmolDevice<ACTIVE> {
    fn rx_post(&mut self, count: usize) {
        let count = core::cmp::min(count, ACTIVE);
        unsafe {
            let avail = &mut *self.rx_avail;
            avail.flags = 0;
            for i in 0..count {
                let d = &mut *self.rx_desc.add(i);
                d.addr = self.rx_buf_pa[i];
                d.len = 4096;
                d.flags = VIRTQ_DESC_F_WRITE;
                d.next = 0;
                avail.ring[i] = i as u16;
            }
            avail.idx = count as u16;
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
        unsafe { &*self.dev }.notify_queue(0);
    }

    fn rx_poll(&mut self) -> Option<(usize, usize)> {
        unsafe {
            let used = &*self.rx_used;
            let used_idx = core::ptr::read_volatile(&used.idx);
            if used_idx == self.rx_last_used {
                return None;
            }
            let elem = used.ring[(self.rx_last_used as usize) % Q_LEN];
            self.rx_last_used = self.rx_last_used.wrapping_add(1);
            Some((elem.id as usize, elem.len as usize))
        }
    }

    fn rx_requeue(&mut self, id: usize) {
        unsafe {
            let avail = &mut *self.rx_avail;
            let idx = avail.idx as usize;
            avail.ring[idx % Q_LEN] = id as u16;
            avail.idx = avail.idx.wrapping_add(1);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
        unsafe { &*self.dev }.notify_queue(0);
    }

    fn tx_poll_reclaim(&mut self) {
        unsafe {
            let used = &*self.tx_used;
            let used_idx = core::ptr::read_volatile(&used.idx);
            while self.tx_last_used != used_idx {
                let elem = used.ring[(self.tx_last_used as usize) % Q_LEN];
                self.tx_last_used = self.tx_last_used.wrapping_add(1);
                let id = elem.id as usize;
                if id < ACTIVE {
                    self.tx_free[id] = true;
                }
            }
        }
    }

    fn tx_send(&mut self, frame: &[u8]) -> bool {
        self.tx_poll_reclaim();
        let mut slot: Option<usize> = None;
        for i in 0..ACTIVE {
            if self.tx_free[i] {
                slot = Some(i);
                self.tx_free[i] = false;
                break;
            }
        }
        let Some(i) = slot else { return false };

        const HDR_LEN: usize = 10;
        if frame.len() + HDR_LEN > 4096 {
            self.tx_free[i] = true;
            return false;
        }
        unsafe {
            for b in 0..HDR_LEN {
                core::ptr::write_volatile((self.tx_buf_va[i] + b) as *mut u8, 0);
            }
            core::ptr::copy_nonoverlapping(
                frame.as_ptr(),
                (self.tx_buf_va[i] + HDR_LEN) as *mut u8,
                frame.len(),
            );
            let d = &mut *self.tx_desc.add(i);
            d.addr = self.tx_buf_pa[i];
            d.len = (HDR_LEN + frame.len()) as u32;
            d.flags = 0;
            d.next = 0;

            let avail = &mut *self.tx_avail;
            let idx = avail.idx as usize;
            avail.ring[idx % Q_LEN] = i as u16;
            avail.idx = avail.idx.wrapping_add(1);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
        unsafe { &*self.dev }.notify_queue(1);
        true
    }
}

struct SmolRx<'a, const ACTIVE: usize> {
    dev: *mut SmolDevice<ACTIVE>,
    id: usize,
    len: usize,
    _lt: core::marker::PhantomData<&'a mut ()>,
}

impl<'a, const ACTIVE: usize> RxToken for SmolRx<'a, ACTIVE> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        const HDR_LEN: usize = 10;
        let d = unsafe { &mut *self.dev };
        let payload_len = self.len.saturating_sub(HDR_LEN).min(4096 - HDR_LEN);
        let payload = unsafe {
            core::slice::from_raw_parts_mut(
                (d.rx_buf_va[self.id] + HDR_LEN) as *mut u8,
                payload_len,
            )
        };
        let r = f(payload);
        d.rx_requeue(self.id);
        r
    }
}

struct SmolTx<'a, const ACTIVE: usize> {
    dev: *mut SmolDevice<ACTIVE>,
    _lt: core::marker::PhantomData<&'a mut ()>,
}

impl<'a, const ACTIVE: usize> TxToken for SmolTx<'a, ACTIVE> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = [0u8; 1536];
        let n = core::cmp::min(len, buf.len());
        let r = f(&mut buf[..n]);
        let d = unsafe { &mut *self.dev };
        let _ = d.tx_send(&buf[..n]);
        r
    }
}

impl<const ACTIVE: usize> Device for SmolDevice<ACTIVE> {
    type RxToken<'b>
        = SmolRx<'b, ACTIVE>
    where
        Self: 'b;
    type TxToken<'b>
        = SmolTx<'b, ACTIVE>
    where
        Self: 'b;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if let Some((id, len)) = self.rx_poll() {
            Some((
                SmolRx { dev: self as *mut _, id, len, _lt: core::marker::PhantomData },
                SmolTx { dev: self as *mut _, _lt: core::marker::PhantomData },
            ))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(SmolTx { dev: self as *mut _, _lt: core::marker::PhantomData })
    }
}

// ——— nexus-net facade implementation (UDP/TCP) ———

pub struct OsUdpSocket {
    inner: Rc<RefCell<Inner>>,
    handle: SocketHandle,
    local: NetSocketAddrV4,
}

impl UdpSocket for OsUdpSocket {
    fn local_addr(&self) -> NetSocketAddrV4 {
        self.local
    }

    fn send_to(&mut self, buf: &[u8], remote: NetSocketAddrV4) -> Result<usize, NetError> {
        validate_udp_payload_len(buf.len())?;
        let mut inner = self.inner.borrow_mut();
        let ep = smoltcp::wire::IpEndpoint::new(
            IpAddress::Ipv4(Ipv4Address::from_bytes(&remote.ip.0)),
            remote.port,
        );
        let sock = inner.sockets.get_mut::<smoltcp::socket::udp::Socket>(self.handle);
        if !sock.can_send() {
            return Err(NetError::WouldBlock);
        }
        sock.send_slice(buf, ep).map_err(|_| NetError::NoBufs)?;
        Ok(buf.len())
    }

    fn recv_from(&mut self, buf: &mut [u8]) -> Result<(usize, NetSocketAddrV4), NetError> {
        let mut inner = self.inner.borrow_mut();
        let sock = inner.sockets.get_mut::<smoltcp::socket::udp::Socket>(self.handle);
        if !sock.can_recv() {
            return Err(NetError::WouldBlock);
        }
        let (n, meta) = sock.recv_slice(buf).map_err(|_| NetError::Internal("udp recv"))?;
        let IpAddress::Ipv4(v4) = meta.endpoint.addr;
        let from = NetSocketAddrV4::new(v4.0, meta.endpoint.port);
        Ok((n, from))
    }
}

pub struct OsTcpListener {
    inner: Rc<RefCell<Inner>>,
    local: NetSocketAddrV4,
    handle: SocketHandle,
}

impl TcpListener for OsTcpListener {
    type Stream = OsTcpStream;

    fn local_addr(&self) -> NetSocketAddrV4 {
        self.local
    }

    fn accept(&mut self, deadline: Option<NetInstant>) -> Result<Self::Stream, NetError> {
        let mut inner = self.inner.borrow_mut();
        let now = inner.now;
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        if !sock.is_active() {
            return Err(NetError::WouldBlock);
        }
        Ok(OsTcpStream { inner: Rc::clone(&self.inner), handle: self.handle })
    }
}

pub struct OsTcpStream {
    inner: Rc<RefCell<Inner>>,
    handle: SocketHandle,
}

impl TcpStream for OsTcpStream {
    fn read(&mut self, deadline: Option<NetInstant>, buf: &mut [u8]) -> Result<usize, NetError> {
        let mut inner = self.inner.borrow_mut();
        let now = inner.now;
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        if !sock.is_active() {
            return Err(NetError::Disconnected);
        }
        if !sock.can_recv() {
            return Err(NetError::WouldBlock);
        }
        sock.recv_slice(buf).map_err(|_| NetError::Internal("tcp recv"))
    }

    fn write(&mut self, deadline: Option<NetInstant>, buf: &[u8]) -> Result<usize, NetError> {
        validate_tcp_write_len(buf.len())?;
        let mut inner = self.inner.borrow_mut();
        let now = inner.now;
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        if !sock.is_active() {
            return Err(NetError::NotConnected);
        }
        if !sock.can_send() {
            return Err(NetError::WouldBlock);
        }
        sock.send_slice(buf).map_err(|_| NetError::NoBufs)
    }

    fn close(&mut self) {
        let mut inner = self.inner.borrow_mut();
        let sock = inner.sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        sock.close();
    }
}

impl NetStack for SmoltcpVirtioNetStack {
    type Udp = OsUdpSocket;
    type TcpListener = OsTcpListener;
    type TcpStream = OsTcpStream;

    fn poll(&mut self, now: NetInstant) {
        let mut inner = self.inner.borrow_mut();
        inner.now = now;

        // Rebuild device wrapper for this poll step.
        let mut devwrap = SmolDevice::<ACTIVE_BUFS> {
            dev: &inner.dev as *const _,
            rx_desc: inner.rx_desc,
            rx_avail: inner.rx_avail,
            rx_used: inner.rx_used,
            rx_last_used: inner.rx_last_used,
            tx_desc: inner.tx_desc,
            tx_avail: inner.tx_avail,
            tx_used: inner.tx_used,
            tx_last_used: inner.tx_last_used,
            rx_buf_va: inner.rx_buf_va,
            rx_buf_pa: inner.rx_buf_pa,
            tx_buf_va: inner.tx_buf_va,
            tx_buf_pa: inner.tx_buf_pa,
            tx_free: inner.tx_free,
        };
        // Split borrows so we can pass iface + sockets mutably in one call.
        let Inner { iface, sockets, .. } = &mut *inner;
        let _ = iface.poll(Instant::from_millis(now as i64), &mut devwrap, sockets);

        // Persist queue state mutated by tokens.
        inner.rx_last_used = devwrap.rx_last_used;
        inner.tx_last_used = devwrap.tx_last_used;
        inner.tx_free = devwrap.tx_free;
    }

    fn next_wake(&self) -> Option<NetInstant> {
        None
    }

    fn udp_bind(&mut self, local: NetSocketAddrV4) -> Result<Self::Udp, NetError> {
        let mut inner = self.inner.borrow_mut();
        let ip = Ipv4Address::from_bytes(&local.ip.0);
        let ep = smoltcp::wire::IpEndpoint::new(IpAddress::Ipv4(ip), local.port);
        let rx = smoltcp::socket::udp::PacketBuffer::new(
            alloc::vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 4],
            alloc::vec![0u8; 2048],
        );
        let tx = smoltcp::socket::udp::PacketBuffer::new(
            alloc::vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 4],
            alloc::vec![0u8; 2048],
        );
        let mut sock = smoltcp::socket::udp::Socket::new(rx, tx);
        sock.bind(ep).map_err(|_| NetError::AddrInUse)?;
        let handle = inner.sockets.add(sock);
        Ok(OsUdpSocket { inner: Rc::clone(&self.inner), handle, local })
    }

    fn tcp_listen(
        &mut self,
        local: NetSocketAddrV4,
        _backlog: usize,
    ) -> Result<Self::TcpListener, NetError> {
        let mut inner = self.inner.borrow_mut();
        // Bring-up sizing: keep per-socket buffers small to avoid exhausting the bump allocator
        // in `nexus-service-entry`.
        let rx = smoltcp::socket::tcp::SocketBuffer::new(alloc::vec![0u8; 1024]);
        let tx = smoltcp::socket::tcp::SocketBuffer::new(alloc::vec![0u8; 1024]);
        let mut sock = smoltcp::socket::tcp::Socket::new(rx, tx);
        sock.set_keep_alive(Some(smoltcp::time::Duration::from_secs(2)));
        sock.listen(local.port).map_err(|_| NetError::AddrInUse)?;
        let handle = inner.sockets.add(sock);
        Ok(OsTcpListener { inner: Rc::clone(&self.inner), local, handle })
    }

    fn tcp_connect(
        &mut self,
        remote: NetSocketAddrV4,
        deadline: Option<NetInstant>,
    ) -> Result<Self::TcpStream, NetError> {
        let mut inner = self.inner.borrow_mut();
        let now = inner.now;
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let rx = smoltcp::socket::tcp::SocketBuffer::new(alloc::vec![0u8; 8192]);
        let tx = smoltcp::socket::tcp::SocketBuffer::new(alloc::vec![0u8; 8192]);
        let mut sock = smoltcp::socket::tcp::Socket::new(rx, tx);
        let local_ip = Ipv4Address::new(10, 0, 2, 15);
        let remote_ip = Ipv4Address::from_bytes(&remote.ip.0);
        // smoltcp requires an interface context for connect.
        let cx = inner.iface.context();
        // Deterministic ephemeral port (bring-up): avoid relying on "0 means ephemeral".
        sock.connect(cx, (local_ip, 40_001), (remote_ip, remote.port))
            .map_err(|_| NetError::InvalidInput("tcp connect"))?;
        let handle = inner.sockets.add(sock);
        Ok(OsTcpStream { inner: Rc::clone(&self.inner), handle })
    }
}
