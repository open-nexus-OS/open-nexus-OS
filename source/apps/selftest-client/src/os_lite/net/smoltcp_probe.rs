// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Optional smoltcp bring-up probe over virtio-net (bounded,
//!   deterministic-ish). Feature-gated (`smoltcp-probe`) to avoid drift and
//!   unused-code warnings. The OS selftest uses `netstackd` for networking
//!   by default; this probe is only enabled for low-level virtio/smoltcp
//!   bring-up debugging.
//! OWNERS: @runtime
//! STATUS: Diagnostic / opt-in
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Not part of the default QEMU marker ladder; manual bring-up
//!   debugging only (`--features smoltcp-probe`).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec;

use net_virtio::{VirtioNetMmio, VIRTIO_DEVICE_ID_NET, VIRTIO_MMIO_MAGIC};
use nexus_abi::yield_;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};

use super::super::mmio::MmioBus;
use crate::markers::{emit_byte, emit_bytes, emit_line, emit_u64};

const VIRTQ_DESC_F_NEXT: u16 = 1;
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

struct VirtioQueues<const N: usize> {
    // RX
    rx_desc: *mut VqDesc,
    rx_avail: *mut VqAvail<N>,
    rx_used: *mut VqUsed<N>,
    rx_last_used: u16,
    // TX
    tx_desc: *mut VqDesc,
    tx_avail: *mut VqAvail<N>,
    tx_used: *mut VqUsed<N>,
    tx_last_used: u16,

    // Buffers (one page each, includes virtio-net hdr prefix).
    rx_buf_va: [usize; N],
    rx_buf_pa: [u64; N],
    tx_buf_va: [usize; N],
    tx_buf_pa: [u64; N],

    // Free TX descriptors.
    tx_free: [bool; N],

    // Minimal diagnostics (bounded, no allocation).
    rx_packets: u32,
    tx_packets: u32,
    tx_drops: u32,
}

impl<const N: usize> VirtioQueues<N> {
    fn rx_replenish(&mut self, dev: &VirtioNetMmio<MmioBus>, count: usize) {
        // Post the first `count` RX buffers once.
        let count = core::cmp::min(count, N);
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
        dev.notify_queue(0);
    }

    fn rx_poll(&mut self) -> Option<(usize, usize)> {
        unsafe {
            let used = &*self.rx_used;
            let used_idx = core::ptr::read_volatile(&used.idx);
            if used_idx == self.rx_last_used {
                return None;
            }
            let elem = used.ring[(self.rx_last_used as usize) % N];
            self.rx_last_used = self.rx_last_used.wrapping_add(1);
            let id = elem.id as usize;
            let len = elem.len as usize;
            self.rx_packets = self.rx_packets.saturating_add(1);
            Some((id, len))
        }
    }

    fn rx_requeue(&mut self, dev: &VirtioNetMmio<MmioBus>, id: usize) {
        unsafe {
            let avail = &mut *self.rx_avail;
            let idx = avail.idx as usize;
            avail.ring[idx % N] = id as u16;
            avail.idx = avail.idx.wrapping_add(1);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
        dev.notify_queue(0);
    }

    fn tx_poll_reclaim(&mut self) {
        unsafe {
            let used = &*self.tx_used;
            let used_idx = core::ptr::read_volatile(&used.idx);
            while self.tx_last_used != used_idx {
                let elem = used.ring[(self.tx_last_used as usize) % N];
                self.tx_last_used = self.tx_last_used.wrapping_add(1);
                let id = elem.id as usize;
                if id < N {
                    self.tx_free[id] = true;
                }
            }
        }
    }

    fn tx_send(&mut self, dev: &VirtioNetMmio<MmioBus>, frame: &[u8]) -> bool {
        self.tx_poll_reclaim();
        let mut slot: Option<usize> = None;
        for i in 0..N {
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
            // zero header
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
            avail.ring[idx % N] = i as u16;
            avail.idx = avail.idx.wrapping_add(1);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
        dev.notify_queue(1);
        self.tx_packets = self.tx_packets.saturating_add(1);
        true
    }
}

struct SmolVirtio<const N: usize> {
    dev: *const VirtioNetMmio<MmioBus>,
    q: *mut VirtioQueues<N>,
}

struct SmolRxToken<'a, const N: usize> {
    dev: *const VirtioNetMmio<MmioBus>,
    q: *mut VirtioQueues<N>,
    id: usize,
    len: usize,
    _lt: core::marker::PhantomData<&'a mut ()>,
}

impl<'a, const N: usize> RxToken for SmolRxToken<'a, N> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        const HDR_LEN: usize = 10;
        let q = unsafe { &mut *self.q };
        let dev = unsafe { &*self.dev };
        let payload_len = self.len.saturating_sub(HDR_LEN).min(4096 - HDR_LEN);
        let payload = unsafe {
            core::slice::from_raw_parts_mut(
                (q.rx_buf_va[self.id] + HDR_LEN) as *mut u8,
                payload_len,
            )
        };
        let r = f(payload);
        q.rx_requeue(dev, self.id);
        r
    }
}

struct SmolTxToken<'a, const N: usize> {
    dev: *const VirtioNetMmio<MmioBus>,
    q: *mut VirtioQueues<N>,
    _lt: core::marker::PhantomData<&'a mut ()>,
}

impl<'a, const N: usize> TxToken for SmolTxToken<'a, N> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // Provide a temporary buffer backed by a stack scratch, then transmit.
        // This keeps borrow/lifetime simple for bring-up.
        let mut buf = [0u8; 1536];
        let n = core::cmp::min(len, buf.len());
        let r = f(&mut buf[..n]);
        let q = unsafe { &mut *self.q };
        let dev = unsafe { &*self.dev };
        if !q.tx_send(dev, &buf[..n]) {
            q.tx_drops = q.tx_drops.saturating_add(1);
        }
        r
    }
}

impl<const N: usize> Device for SmolVirtio<N> {
    type RxToken<'b>
        = SmolRxToken<'b, N>
    where
        Self: 'b;
    type TxToken<'b>
        = SmolTxToken<'b, N>
    where
        Self: 'b;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let q = unsafe { &mut *self.q };
        if let Some((id, len)) = q.rx_poll() {
            Some((
                SmolRxToken { dev: self.dev, q: self.q, id, len, _lt: core::marker::PhantomData },
                SmolTxToken { dev: self.dev, q: self.q, _lt: core::marker::PhantomData },
            ))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(SmolTxToken { dev: self.dev, q: self.q, _lt: core::marker::PhantomData })
    }
}

#[allow(dead_code)]
pub(crate) fn smoltcp_ping_probe() -> core::result::Result<(), ()> {
    // Minimal bring-up: create an interface and attempt an ICMP echo to the QEMU usernet gateway.
    //
    // NOTE: This is best-effort and bounded; the marker is emitted only on success.
    const MMIO_CAP_SLOT: u32 = 48;
    const MMIO_VA: usize = 0x2000_e000;
    // NOTE: `mmio_map_probe()` may have already mapped this window earlier in the selftest.
    // Treat InvalidArgument as "already mapped" rather than a hard failure.
    let mmio_map_ok = |va: usize, off: usize| -> core::result::Result<(), ()> {
        match nexus_abi::mmio_map(MMIO_CAP_SLOT, va, off) {
            Ok(()) => Ok(()),
            Err(nexus_abi::AbiError::InvalidArgument) => Ok(()),
            Err(_) => Err(()),
        }
    };
    mmio_map_ok(MMIO_VA, 0)?;
    let magic = unsafe { core::ptr::read_volatile((MMIO_VA + 0x000) as *const u32) };
    let device_id = unsafe { core::ptr::read_volatile((MMIO_VA + 0x008) as *const u32) };
    if magic != VIRTIO_MMIO_MAGIC || device_id != VIRTIO_DEVICE_ID_NET {
        emit_line("SELFTEST: smoltcp no virtio-net");
        return Err(());
    }
    let dev = VirtioNetMmio::new(MmioBus { base: MMIO_VA });
    if dev.probe().is_err() {
        emit_line("SELFTEST: smoltcp probe FAIL");
        return Err(());
    }
    // Do NOT reset/re-negotiate here: mmio_map_probe already brought the device up, and
    // we must not invalidate earlier "net up" markers in the same selftest run.

    // Read MAC from virtio-net config space (offset 0x100).
    // NOTE(task-0023b cut 6): pre-existing typo in dead code (`dev_va`) replaced with the
    // already-mapped `MMIO_VA` so the `smoltcp-probe` cfg-gate compiles per RFC-0038.
    let mac = {
        let w0 = unsafe { core::ptr::read_volatile((MMIO_VA + 0x100) as *const u32) };
        let w1 = unsafe { core::ptr::read_volatile((MMIO_VA + 0x104) as *const u32) };
        [
            (w0 & 0xff) as u8,
            ((w0 >> 8) & 0xff) as u8,
            ((w0 >> 16) & 0xff) as u8,
            ((w0 >> 24) & 0xff) as u8,
            (w1 & 0xff) as u8,
            ((w1 >> 8) & 0xff) as u8,
        ]
    };

    // Allocate queue memory and buffers close to existing mappings to avoid kernel PT heap blowups.
    const N: usize = 8;
    const QUEUE_VA: usize = 0x2004_0000;
    const BUF_VA: usize = 0x2006_0000;
    const Q_PAGES_PER_QUEUE: usize = 1;
    const TOTAL_Q_PAGES: usize = Q_PAGES_PER_QUEUE * 2; // rx+tx

    let q_vmo = match nexus_abi::vmo_create(TOTAL_Q_PAGES * 4096) {
        Ok(v) => v,
        Err(_) => {
            emit_line("SELFTEST: smoltcp qvmo FAIL");
            return Err(());
        }
    };
    let flags = nexus_abi::page_flags::VALID
        | nexus_abi::page_flags::USER
        | nexus_abi::page_flags::READ
        | nexus_abi::page_flags::WRITE;
    for page in 0..TOTAL_Q_PAGES {
        let va = QUEUE_VA + page * 4096;
        let off = page * 4096;
        if nexus_abi::vmo_map_page(q_vmo, va, off, flags).is_err() {
            emit_line("SELFTEST: smoltcp qmap FAIL");
            return Err(());
        }
    }
    let mut q_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    if nexus_abi::cap_query(q_vmo, &mut q_info).is_err() {
        emit_line("SELFTEST: smoltcp qquery FAIL");
        return Err(());
    }
    let q_base_pa = q_info.base;

    // Layout for legacy (queue_align=4): desc at base, then avail, then used (same page).
    let align4 = |x: usize| (x + 3) & !3usize;
    let rx_desc_va = QUEUE_VA + 0;
    let rx_avail_va = rx_desc_va + core::mem::size_of::<VqDesc>() * N;
    let rx_used_va = rx_desc_va
        + align4(core::mem::size_of::<VqDesc>() * N + core::mem::size_of::<VqAvail<N>>());
    let tx_desc_va = QUEUE_VA + Q_PAGES_PER_QUEUE * 4096;
    let tx_avail_va = tx_desc_va + core::mem::size_of::<VqDesc>() * N;
    let tx_used_va = tx_desc_va
        + align4(core::mem::size_of::<VqDesc>() * N + core::mem::size_of::<VqAvail<N>>());

    let rx_desc_pa = q_base_pa + 0;
    let tx_desc_pa = q_base_pa + (Q_PAGES_PER_QUEUE as u64) * 4096;

    // Setup queues (legacy uses PFN of desc base).
    if dev
        .setup_queue(
            0,
            &net_virtio::QueueSetup {
                size: N as u16,
                desc_paddr: rx_desc_pa,
                avail_paddr: 0,
                used_paddr: 0,
            },
        )
        .is_err()
    {
        emit_line("SELFTEST: smoltcp q0 FAIL");
        return Err(());
    }
    if dev
        .setup_queue(
            1,
            &net_virtio::QueueSetup {
                size: N as u16,
                desc_paddr: tx_desc_pa,
                avail_paddr: 0,
                used_paddr: 0,
            },
        )
        .is_err()
    {
        emit_line("SELFTEST: smoltcp q1 FAIL");
        return Err(());
    }

    // Buffers: N rx + N tx pages.
    let buf_vmo = match nexus_abi::vmo_create((N * 2) * 4096) {
        Ok(v) => v,
        Err(_) => {
            emit_line("SELFTEST: smoltcp bvmo FAIL");
            return Err(());
        }
    };
    for page in 0..(N * 2) {
        let va = BUF_VA + page * 4096;
        let off = page * 4096;
        if nexus_abi::vmo_map_page(buf_vmo, va, off, flags).is_err() {
            emit_line("SELFTEST: smoltcp bmap FAIL");
            return Err(());
        }
    }
    let mut bq = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    if nexus_abi::cap_query(buf_vmo, &mut bq).is_err() {
        emit_line("SELFTEST: smoltcp bquery FAIL");
        return Err(());
    }

    let mut q = VirtioQueues::<N> {
        rx_desc: rx_desc_va as *mut VqDesc,
        rx_avail: rx_avail_va as *mut VqAvail<N>,
        rx_used: rx_used_va as *mut VqUsed<N>,
        rx_last_used: 0,
        tx_desc: tx_desc_va as *mut VqDesc,
        tx_avail: tx_avail_va as *mut VqAvail<N>,
        tx_used: tx_used_va as *mut VqUsed<N>,
        tx_last_used: 0,
        rx_buf_va: [0; N],
        rx_buf_pa: [0; N],
        tx_buf_va: [0; N],
        tx_buf_pa: [0; N],
        tx_free: [true; N],
        rx_packets: 0,
        tx_packets: 0,
        tx_drops: 0,
    };
    for i in 0..N {
        q.rx_buf_va[i] = BUF_VA + i * 4096;
        q.rx_buf_pa[i] = bq.base + (i as u64) * 4096;
        q.tx_buf_va[i] = BUF_VA + (N + i) * 4096;
        q.tx_buf_pa[i] = bq.base + ((N + i) as u64) * 4096;
    }
    // Zero rings
    unsafe {
        core::ptr::write_bytes(QUEUE_VA as *mut u8, 0, TOTAL_Q_PAGES * 4096);
    }
    q.rx_replenish(&dev, N);
    dev.set_driver_ok();

    // smoltcp iface
    let hw = HardwareAddress::Ethernet(EthernetAddress(mac));
    let mut cfg = smoltcp::iface::Config::new(hw);
    cfg.random_seed = 0x1234_5678;
    let mut phy = SmolVirtio::<N> { dev: &dev as *const _, q: &mut q as *mut _ };
    let mut iface = smoltcp::iface::Interface::new(cfg, &mut phy, Instant::from_millis(0));
    iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 15)), 24)).ok();
    });
    // Route to the QEMU usernet gateway.
    if iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).is_err() {
        emit_line("SELFTEST: smoltcp route FAIL");
        return Err(());
    }

    // ICMP socket
    let rx_meta = [smoltcp::socket::icmp::PacketMetadata::EMPTY; 4];
    let tx_meta = [smoltcp::socket::icmp::PacketMetadata::EMPTY; 4];
    let rx_buf = smoltcp::socket::icmp::PacketBuffer::new(rx_meta, vec![0u8; 256]);
    let tx_buf = smoltcp::socket::icmp::PacketBuffer::new(tx_meta, vec![0u8; 256]);
    let mut icmp = smoltcp::socket::icmp::Socket::new(rx_buf, tx_buf);
    if icmp.bind(smoltcp::socket::icmp::Endpoint::Ident(0x1234)).is_err() {
        emit_line("SELFTEST: smoltcp bind FAIL");
        return Err(());
    }
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    let handle = sockets.add(icmp);

    let target = IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 2));
    let checksum = smoltcp::phy::ChecksumCapabilities::default();
    let mut sent = false;
    let mut send_err = false;
    // Bounded poll loop.
    for _ in 0..2000 {
        let now_ns = nexus_abi::nsec().map_err(|_| ())?;
        let ts = Instant::from_millis((now_ns / 1_000_000) as i64);
        {
            let _ = iface.poll(ts, &mut phy, &mut sockets);
        }
        {
            let sock = sockets.get_mut::<smoltcp::socket::icmp::Socket>(handle);
            if !sent && sock.can_send() {
                // Craft an ICMPv4 EchoRequest packet and send it.
                let mut bytes = [0u8; 24]; // 8 header + 16 payload
                let mut pkt = smoltcp::wire::Icmpv4Packet::new_unchecked(&mut bytes);
                let repr = smoltcp::wire::Icmpv4Repr::EchoRequest {
                    ident: 0x1234,
                    seq_no: 1,
                    data: &[0u8; 16],
                };
                repr.emit(&mut pkt, &checksum);
                if sock.send_slice(pkt.into_inner(), target).is_err() {
                    send_err = true;
                }
                sent = true;
            }
            if sock.can_recv() {
                let _ = sock.recv();
                return Ok(());
            }
        }
        let _ = yield_();
    }
    if send_err {
        emit_line("SELFTEST: smoltcp send FAIL");
    }
    emit_bytes(b"SELFTEST: smoltcp diag rx=");
    emit_u64(q.rx_packets as u64);
    emit_bytes(b" tx=");
    emit_u64(q.tx_packets as u64);
    emit_bytes(b" drop=");
    emit_u64(q.tx_drops as u64);
    emit_byte(b'\n');
    Err(())
}
