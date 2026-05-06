// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Minimal virtio-input MMIO driver layer for QEMU `virt` live input.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Focused host probe/decode tests in this crate; OS/QEMU proof via `hidrawd`.
//!
//! ADR: docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md

#![cfg_attr(all(feature = "os-lite", not(feature = "std")), no_std)]
#![cfg_attr(not(all(feature = "os-lite", not(feature = "std"))), forbid(unsafe_code))]

#[cfg(all(feature = "os-lite", not(feature = "std")))]
extern crate alloc;

#[cfg(all(feature = "os-lite", not(feature = "std")))]
use alloc::vec::Vec;
use nexus_hal::Bus;
#[cfg(feature = "std")]
use std::vec::Vec;

#[cfg(all(feature = "os-lite", not(feature = "std")))]
use nexus_abi::{cap_query, mmio_map, vmo_create, vmo_map_page, AbiError, CapQuery};

pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
pub const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
pub const VIRTIO_DEVICE_ID_INPUT: u32 = 18;
pub const INPUT_EVENT_QUEUE_INDEX: u32 = 0;
pub const DEFAULT_QUEUE_ENTRIES: u16 = 32;

const REG_MAGIC: usize = 0x000;
const REG_VERSION: usize = 0x004;
const REG_DEVICE_ID: usize = 0x008;
const REG_DEVICE_FEATURES: usize = 0x010;
const REG_DEVICE_FEATURES_SEL: usize = 0x014;
const REG_DRIVER_FEATURES: usize = 0x020;
const REG_DRIVER_FEATURES_SEL: usize = 0x024;
const REG_QUEUE_SEL: usize = 0x030;
const REG_QUEUE_NUM_MAX: usize = 0x034;
const REG_QUEUE_NUM: usize = 0x038;
const REG_QUEUE_READY: usize = 0x044;
const REG_QUEUE_NOTIFY: usize = 0x050;
const REG_STATUS: usize = 0x070;
const REG_QUEUE_DESC_LOW: usize = 0x080;
const REG_QUEUE_DESC_HIGH: usize = 0x084;
const REG_QUEUE_DRIVER_LOW: usize = 0x090;
const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;
const REG_CONFIG_SELECT: usize = 0x0fc;
const REG_CONFIG_SUBSEL: usize = 0x100;
const REG_CONFIG_SIZE: usize = 0x104;
const REG_CONFIG_DATA: usize = 0x108;

const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;
const STATUS_FAILED: u32 = 128;

const VIRTQ_DESC_F_WRITE: u16 = 2;
const CONFIG_SELECT_EV_BITS: u32 = 0x11;
const CONFIG_SELECT_ABS_INFO: u32 = 0x12;
const EVENT_TYPE_SYN: u16 = 0x00;
const EVENT_TYPE_KEY: u16 = 0x01;
const EVENT_TYPE_REL: u16 = 0x02;
const EVENT_TYPE_ABS: u16 = 0x03;
const SYN_REPORT_CODE: u16 = 0;
const ABS_INFO_LEN: usize = 20;
const INPUT_EVENT_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceSlot(u8);

impl DeviceSlot {
    #[must_use]
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRole {
    Keyboard,
    RelativePointer,
    AbsolutePointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEventKind {
    Syn,
    Key,
    Relative,
    Absolute,
    Unknown(u16),
}

impl InputEventKind {
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            EVENT_TYPE_SYN => Self::Syn,
            EVENT_TYPE_KEY => Self::Key,
            EVENT_TYPE_REL => Self::Relative,
            EVENT_TYPE_ABS => Self::Absolute,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsoluteAxisInfo {
    min: i32,
    max: i32,
}

impl AbsoluteAxisInfo {
    #[must_use]
    pub const fn new(min: i32, max: i32) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub const fn min(self) -> i32 {
        self.min
    }

    #[must_use]
    pub const fn max(self) -> i32 {
        self.max
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawInputEvent {
    event_type: u16,
    code: u16,
    value: i32,
}

impl RawInputEvent {
    #[must_use]
    pub const fn new(event_type: u16, code: u16, value: i32) -> Self {
        Self { event_type, code, value }
    }

    #[must_use]
    pub fn from_le_bytes(bytes: [u8; INPUT_EVENT_SIZE]) -> Self {
        let event_type = u16::from_le_bytes([bytes[0], bytes[1]]);
        let code = u16::from_le_bytes([bytes[2], bytes[3]]);
        let value = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        Self { event_type, code, value }
    }

    #[must_use]
    pub const fn kind(self) -> InputEventKind {
        InputEventKind::from_raw(self.event_type)
    }

    #[must_use]
    pub const fn event_type(self) -> u16 {
        self.event_type
    }

    #[must_use]
    pub const fn code(self) -> u16 {
        self.code
    }

    #[must_use]
    pub const fn value(self) -> i32 {
        self.value
    }

    #[must_use]
    pub const fn is_syn_report(self) -> bool {
        self.event_type == EVENT_TYPE_SYN && self.code == SYN_REPORT_CODE
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolledBatch {
    slot: DeviceSlot,
    role: DeviceRole,
    events: Vec<RawInputEvent>,
}

impl PolledBatch {
    #[must_use]
    pub fn new(slot: DeviceSlot, role: DeviceRole, events: Vec<RawInputEvent>) -> Self {
        Self { slot, role, events }
    }

    #[must_use]
    pub const fn slot(&self) -> DeviceSlot {
        self.slot
    }

    #[must_use]
    pub const fn role(&self) -> DeviceRole {
        self.role
    }

    #[must_use]
    pub fn events(&self) -> &[RawInputEvent] {
        self.events.as_slice()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "virtio-input errors must be handled"]
pub enum VirtioInputError {
    BadMagic,
    UnsupportedVersion,
    NotInputDevice,
    QueueUnavailable,
    QueueTooSmall,
    DeviceRejectedFeatures,
    ConfigUnavailable,
    MapFailed,
    InvalidDescriptor,
}

impl core::fmt::Display for VirtioInputError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "invalid virtio-mmio magic"),
            Self::UnsupportedVersion => write!(f, "unsupported virtio-mmio version"),
            Self::NotInputDevice => write!(f, "virtio-mmio device is not input"),
            Self::QueueUnavailable => write!(f, "virtio-input queue unavailable"),
            Self::QueueTooSmall => write!(f, "virtio-input queue too small"),
            Self::DeviceRejectedFeatures => write!(f, "virtio-input rejected features"),
            Self::ConfigUnavailable => write!(f, "virtio-input config unavailable"),
            Self::MapFailed => write!(f, "virtio-input mmio/vmo mapping failed"),
            Self::InvalidDescriptor => write!(f, "virtio-input descriptor invalid"),
        }
    }
}

pub struct VirtioInputMmio<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioInputMmio<B> {
    #[must_use]
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    pub fn probe(&self) -> Result<(), VirtioInputError> {
        if self.bus.read(REG_MAGIC) != VIRTIO_MMIO_MAGIC {
            return Err(VirtioInputError::BadMagic);
        }
        if self.bus.read(REG_VERSION) != VIRTIO_MMIO_VERSION_MODERN {
            return Err(VirtioInputError::UnsupportedVersion);
        }
        if self.bus.read(REG_DEVICE_ID) != VIRTIO_DEVICE_ID_INPUT {
            return Err(VirtioInputError::NotInputDevice);
        }
        Ok(())
    }

    pub fn negotiate_features_none(&self) -> Result<(), VirtioInputError> {
        self.bus.write(REG_STATUS, 0);
        self.bus.write(REG_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        self.bus.write(REG_DEVICE_FEATURES_SEL, 0);
        let _ = self.bus.read(REG_DEVICE_FEATURES);
        self.bus.write(REG_DEVICE_FEATURES_SEL, 1);
        let _ = self.bus.read(REG_DEVICE_FEATURES);
        self.bus.write(REG_DRIVER_FEATURES_SEL, 0);
        self.bus.write(REG_DRIVER_FEATURES, 0);
        self.bus.write(REG_DRIVER_FEATURES_SEL, 1);
        self.bus.write(REG_DRIVER_FEATURES, 0);
        let status = self.bus.read(REG_STATUS);
        self.bus.write(REG_STATUS, status | STATUS_FEATURES_OK);
        if self.bus.read(REG_STATUS) & STATUS_FEATURES_OK == 0 {
            self.bus.write(REG_STATUS, self.bus.read(REG_STATUS) | STATUS_FAILED);
            return Err(VirtioInputError::DeviceRejectedFeatures);
        }
        Ok(())
    }

    pub fn detect_role(&self) -> Result<DeviceRole, VirtioInputError> {
        let rel_bits = self
            .read_config_bytes(CONFIG_SELECT_EV_BITS, EVENT_TYPE_REL as u8)
            .or_else(config_unavailable_as_empty)?;
        if bit_is_set(&rel_bits, 0) || bit_is_set(&rel_bits, 1) {
            return Ok(DeviceRole::RelativePointer);
        }
        let abs_bits = self
            .read_config_bytes(CONFIG_SELECT_EV_BITS, EVENT_TYPE_ABS as u8)
            .or_else(config_unavailable_as_empty)?;
        if bit_is_set(&abs_bits, 0) || bit_is_set(&abs_bits, 1) {
            return Ok(DeviceRole::AbsolutePointer);
        }
        Ok(DeviceRole::Keyboard)
    }

    pub fn read_absolute_axis_info(&self, axis_code: u8) -> Result<AbsoluteAxisInfo, VirtioInputError> {
        let bytes = self.read_config_bytes(CONFIG_SELECT_ABS_INFO, axis_code)?;
        if bytes.len() < ABS_INFO_LEN {
            return Err(VirtioInputError::ConfigUnavailable);
        }
        let min = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let max = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        Ok(AbsoluteAxisInfo::new(min, max))
    }

    fn read_config_bytes(&self, select: u32, subsel: u8) -> Result<Vec<u8>, VirtioInputError> {
        self.bus.write(REG_CONFIG_SELECT, select);
        self.bus.write(REG_CONFIG_SUBSEL, u32::from(subsel));
        let size = self.bus.read(REG_CONFIG_SIZE) as usize;
        if size == 0 {
            return Err(VirtioInputError::ConfigUnavailable);
        }
        let mut out = Vec::with_capacity(size);
        for idx in 0..size {
            let word = self.bus.read(REG_CONFIG_DATA + (idx & !0x3));
            let byte = ((word >> ((idx & 0x3) * 8)) & 0xff) as u8;
            out.push(byte);
        }
        Ok(out)
    }
}

fn bit_is_set(bytes: &[u8], bit: usize) -> bool {
    let byte = bit / 8;
    let mask = 1u8 << (bit % 8);
    bytes.get(byte).copied().unwrap_or(0) & mask != 0
}

fn config_unavailable_as_empty(err: VirtioInputError) -> Result<Vec<u8>, VirtioInputError> {
    match err {
        VirtioInputError::ConfigUnavailable => Ok(Vec::new()),
        other => Err(other),
    }
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
#[repr(C)]
#[derive(Clone, Copy)]
struct VqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
#[repr(C)]
struct VqAvail<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [u16; N],
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
#[repr(C)]
#[derive(Clone, Copy)]
struct VqUsedElem {
    id: u32,
    len: u32,
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
#[repr(C)]
struct VqUsed<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [VqUsedElem; N],
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
#[derive(Clone, Copy)]
struct MmioBus {
    base_va: usize,
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
impl MmioBus {
    const fn new(base_va: usize) -> Self {
        Self { base_va }
    }
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
impl Bus for MmioBus {
    fn read(&self, addr: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base_va + addr) as *const u32) }
    }

    fn write(&self, addr: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.base_va + addr) as *mut u32, value) }
    }
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
pub struct MappedVirtioInputDevice {
    slot: DeviceSlot,
    role: DeviceRole,
    bus: MmioBus,
    queue: QueueState<{ DEFAULT_QUEUE_ENTRIES as usize }>,
    absolute_x: Option<AbsoluteAxisInfo>,
    absolute_y: Option<AbsoluteAxisInfo>,
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
impl MappedVirtioInputDevice {
    pub fn open(
        mmio_cap_slot: u32,
        mmio_base_va: usize,
        queue_va: usize,
        buffer_va: usize,
        slot: DeviceSlot,
    ) -> Result<Self, VirtioInputError> {
        mmio_map(mmio_cap_slot, mmio_base_va, 0)
            .or_else(|err| if err == AbiError::InvalidArgument { Ok(()) } else { Err(err) })
            .map_err(|_| VirtioInputError::MapFailed)?;
        let bus = MmioBus::new(mmio_base_va);
        let mmio = VirtioInputMmio::new(bus);
        mmio.probe()?;
        mmio.negotiate_features_none()?;
        let role = mmio.detect_role()?;
        let absolute_x = if role == DeviceRole::AbsolutePointer {
            mmio.read_absolute_axis_info(0).ok()
        } else {
            None
        };
        let absolute_y = if role == DeviceRole::AbsolutePointer {
            mmio.read_absolute_axis_info(1).ok()
        } else {
            None
        };
        let queue = QueueState::<{ DEFAULT_QUEUE_ENTRIES as usize }>::new(
            &bus,
            queue_va,
            buffer_va,
            INPUT_EVENT_QUEUE_INDEX,
        )?;
        bus.write(REG_STATUS, bus.read(REG_STATUS) | STATUS_DRIVER_OK);
        bus.write(REG_QUEUE_NOTIFY, INPUT_EVENT_QUEUE_INDEX);
        Ok(Self { slot, role, bus, queue, absolute_x, absolute_y })
    }

    #[must_use]
    pub const fn slot(&self) -> DeviceSlot {
        self.slot
    }

    #[must_use]
    pub const fn role(&self) -> DeviceRole {
        self.role
    }

    #[must_use]
    pub const fn absolute_x(&self) -> Option<AbsoluteAxisInfo> {
        self.absolute_x
    }

    #[must_use]
    pub const fn absolute_y(&self) -> Option<AbsoluteAxisInfo> {
        self.absolute_y
    }

    pub fn poll_batch(&mut self) -> Result<Option<PolledBatch>, VirtioInputError> {
        let mut events = Vec::new();
        let mut requeued = false;
        while let Some((desc_id, raw_event)) = self.queue.take_used_event()? {
            if !raw_event.is_syn_report() {
                events.push(raw_event);
            }
            self.queue.requeue_desc(desc_id, &self.bus)?;
            requeued = true;
        }
        if requeued {
            self.bus.write(REG_QUEUE_NOTIFY, INPUT_EVENT_QUEUE_INDEX);
        }
        if events.is_empty() {
            return Ok(None);
        }
        Ok(Some(PolledBatch::new(self.slot, self.role, events)))
    }
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
struct QueueState<const N: usize> {
    _queue_vmo: u32,
    _buffer_vmo: u32,
    queue_va: usize,
    buffer_va: usize,
    _desc_pa: u64,
    last_used_idx: u16,
    next_avail_idx: u16,
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
impl<const N: usize> QueueState<N> {
    fn new(
        bus: &MmioBus,
        queue_va: usize,
        buffer_va: usize,
        queue_index: u32,
    ) -> Result<Self, VirtioInputError> {
        let queue_vmo = vmo_create(4096).map_err(|_| VirtioInputError::MapFailed)?;
        let buffer_vmo = vmo_create(4096).map_err(|_| VirtioInputError::MapFailed)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        vmo_map_page(queue_vmo, queue_va, 0, flags).map_err(|_| VirtioInputError::MapFailed)?;
        vmo_map_page(buffer_vmo, buffer_va, 0, flags).map_err(|_| VirtioInputError::MapFailed)?;
        let mut queue_info = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        cap_query(queue_vmo, &mut queue_info).map_err(|_| VirtioInputError::MapFailed)?;
        let mut buffer_info = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        cap_query(buffer_vmo, &mut buffer_info).map_err(|_| VirtioInputError::MapFailed)?;

        unsafe {
            core::ptr::write_bytes(queue_va as *mut u8, 0, 4096);
            core::ptr::write_bytes(buffer_va as *mut u8, 0, 4096);
        }

        let desc_va = queue_va;
        let avail_va = desc_va + core::mem::size_of::<VqDesc>() * N;
        let used_va =
            desc_va + align4(core::mem::size_of::<VqDesc>() * N + core::mem::size_of::<VqAvail<N>>());
        let avail_pa = queue_info.base + (avail_va - desc_va) as u64;
        let used_pa = queue_info.base + (used_va - desc_va) as u64;

        bus.write(REG_QUEUE_SEL, queue_index);
        let max = bus.read(REG_QUEUE_NUM_MAX);
        if max == 0 {
            return Err(VirtioInputError::QueueUnavailable);
        }
        if max < N as u32 {
            return Err(VirtioInputError::QueueTooSmall);
        }
        bus.write(REG_QUEUE_NUM, N as u32);
        write_u64_mmio_pair(bus, REG_QUEUE_DESC_LOW, REG_QUEUE_DESC_HIGH, queue_info.base);
        write_u64_mmio_pair(bus, REG_QUEUE_DRIVER_LOW, REG_QUEUE_DRIVER_HIGH, avail_pa);
        write_u64_mmio_pair(bus, REG_QUEUE_DEVICE_LOW, REG_QUEUE_DEVICE_HIGH, used_pa);
        bus.write(REG_QUEUE_READY, 1);

        let desc = desc_va as *mut VqDesc;
        let avail = avail_va as *mut VqAvail<N>;
        for idx in 0..N {
            let buf_pa = buffer_info.base + (idx * INPUT_EVENT_SIZE) as u64;
            unsafe {
                core::ptr::write_volatile(&mut (*desc.add(idx)).addr, buf_pa);
                core::ptr::write_volatile(&mut (*desc.add(idx)).len, INPUT_EVENT_SIZE as u32);
                core::ptr::write_volatile(&mut (*desc.add(idx)).flags, VIRTQ_DESC_F_WRITE);
                core::ptr::write_volatile(&mut (*desc.add(idx)).next, 0);
                core::ptr::write_volatile(&mut (*avail).ring[idx], idx as u16);
            }
        }
        unsafe {
            core::ptr::write_volatile(&mut (*avail).flags, 0);
            core::ptr::write_volatile(&mut (*avail).idx, N as u16);
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        Ok(Self {
            _queue_vmo: queue_vmo,
            _buffer_vmo: buffer_vmo,
            queue_va,
            buffer_va,
            _desc_pa: queue_info.base,
            last_used_idx: 0,
            next_avail_idx: N as u16,
        })
    }

    fn take_used_event(&mut self) -> Result<Option<(u16, RawInputEvent)>, VirtioInputError> {
        let used = unsafe { &*(self.used_va() as *const VqUsed<N>) };
        let used_idx = unsafe { core::ptr::read_volatile(&used.idx) };
        if self.last_used_idx == used_idx {
            return Ok(None);
        }
        let ring_idx = usize::from(self.last_used_idx % (N as u16));
        let elem = unsafe { core::ptr::read_volatile(&used.ring[ring_idx]) };
        let desc_id = elem.id as usize;
        if desc_id >= N {
            return Err(VirtioInputError::InvalidDescriptor);
        }
        let offset = desc_id * INPUT_EVENT_SIZE;
        let bytes = unsafe {
            core::slice::from_raw_parts((self.buffer_va + offset) as *const u8, INPUT_EVENT_SIZE)
        };
        let raw_event = RawInputEvent::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Ok(Some((desc_id as u16, raw_event)))
    }

    fn requeue_desc(&mut self, desc_id: u16, bus: &MmioBus) -> Result<(), VirtioInputError> {
        if usize::from(desc_id) >= N {
            return Err(VirtioInputError::InvalidDescriptor);
        }
        let avail = self.avail_va() as *mut VqAvail<N>;
        let ring_idx = usize::from(self.next_avail_idx % (N as u16));
        unsafe {
            core::ptr::write_volatile(&mut (*avail).ring[ring_idx], desc_id);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*avail).idx, self.next_avail_idx.wrapping_add(1));
        }
        self.next_avail_idx = self.next_avail_idx.wrapping_add(1);
        bus.write(REG_QUEUE_NOTIFY, INPUT_EVENT_QUEUE_INDEX);
        Ok(())
    }

    const fn avail_va(&self) -> usize {
        self.queue_va + core::mem::size_of::<VqDesc>() * N
    }

    const fn used_va(&self) -> usize {
        self.queue_va
            + align4(core::mem::size_of::<VqDesc>() * N + core::mem::size_of::<VqAvail<N>>())
    }
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

#[cfg(all(feature = "os-lite", not(feature = "std")))]
fn write_u64_mmio_pair(bus: &MmioBus, low_reg: usize, high_reg: usize, value: u64) {
    bus.write(low_reg, (value & 0xffff_ffff) as u32);
    bus.write(high_reg, (value >> 32) as u32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::{Cell, RefCell};

    struct MockBus {
        magic: u32,
        version: u32,
        device_id: u32,
        select: Cell<u32>,
        subsel: Cell<u32>,
        rel_bits: Option<[u8; 1]>,
        abs_bits: Option<[u8; 1]>,
        abs_x: [u8; ABS_INFO_LEN],
        abs_y: [u8; ABS_INFO_LEN],
        writes: RefCell<Vec<(usize, u32)>>,
    }

    impl MockBus {
        fn keyboard() -> Self {
            Self {
                magic: VIRTIO_MMIO_MAGIC,
                version: VIRTIO_MMIO_VERSION_MODERN,
                device_id: VIRTIO_DEVICE_ID_INPUT,
                select: Cell::new(0),
                subsel: Cell::new(0),
                rel_bits: Some([0]),
                abs_bits: Some([0]),
                abs_x: [0; ABS_INFO_LEN],
                abs_y: [0; ABS_INFO_LEN],
                writes: RefCell::new(Vec::new()),
            }
        }

        fn keyboard_without_optional_configs() -> Self {
            Self {
                rel_bits: None,
                abs_bits: None,
                ..Self::keyboard()
            }
        }

        fn absolute_pointer(max_x: i32, max_y: i32) -> Self {
            let mut abs_x = [0u8; ABS_INFO_LEN];
            let mut abs_y = [0u8; ABS_INFO_LEN];
            abs_x[4..8].copy_from_slice(&max_x.to_le_bytes());
            abs_y[4..8].copy_from_slice(&max_y.to_le_bytes());
            Self {
                abs_bits: Some([0b11]),
                abs_x,
                abs_y,
                ..Self::keyboard()
            }
        }
    }

    impl Bus for MockBus {
        fn read(&self, addr: usize) -> u32 {
            match addr {
                REG_MAGIC => self.magic,
                REG_VERSION => self.version,
                REG_DEVICE_ID => self.device_id,
                REG_STATUS => {
                    let writes = self.writes.borrow();
                    writes
                        .iter()
                        .rev()
                        .find_map(|(offset, value)| (*offset == REG_STATUS).then_some(*value))
                        .unwrap_or(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK)
                }
                REG_CONFIG_SIZE => match (self.select.get(), self.subsel.get()) {
                    (CONFIG_SELECT_EV_BITS, x) if x == EVENT_TYPE_REL as u32 => {
                        self.rel_bits.map_or(0, |bits| bits.len() as u32)
                    }
                    (CONFIG_SELECT_EV_BITS, x) if x == EVENT_TYPE_ABS as u32 => {
                        self.abs_bits.map_or(0, |bits| bits.len() as u32)
                    }
                    (CONFIG_SELECT_ABS_INFO, x) if x == 0 => ABS_INFO_LEN as u32,
                    (CONFIG_SELECT_ABS_INFO, x) if x == 1 => ABS_INFO_LEN as u32,
                    _ => 0,
                },
                offset if (REG_CONFIG_DATA..REG_CONFIG_DATA + ABS_INFO_LEN).contains(&offset) => {
                    let bytes = match (self.select.get(), self.subsel.get()) {
                        (CONFIG_SELECT_EV_BITS, x) if x == EVENT_TYPE_REL as u32 => {
                            self.rel_bits.as_ref().map_or(&[][..], |bits| &bits[..])
                        }
                        (CONFIG_SELECT_EV_BITS, x) if x == EVENT_TYPE_ABS as u32 => {
                            self.abs_bits.as_ref().map_or(&[][..], |bits| &bits[..])
                        }
                        (CONFIG_SELECT_ABS_INFO, x) if x == 0 => &self.abs_x[..],
                        (CONFIG_SELECT_ABS_INFO, x) if x == 1 => &self.abs_y[..],
                        _ => &[],
                    };
                    let base = offset - REG_CONFIG_DATA;
                    let mut word = [0u8; 4];
                    for (idx, slot) in word.iter_mut().enumerate() {
                        *slot = bytes.get(base + idx).copied().unwrap_or(0);
                    }
                    u32::from_le_bytes(word)
                }
                _ => 0,
            }
        }

        fn write(&self, addr: usize, value: u32) {
            if addr == REG_CONFIG_SELECT {
                self.select.set(value);
            } else if addr == REG_CONFIG_SUBSEL {
                self.subsel.set(value);
            }
            self.writes.borrow_mut().push((addr, value));
        }
    }

    #[test]
    fn probe_rejects_bad_magic() {
        let mut bus = MockBus::keyboard();
        bus.magic = 0;
        let dev = VirtioInputMmio::new(bus);
        assert_eq!(dev.probe(), Err(VirtioInputError::BadMagic));
    }

    #[test]
    fn detect_role_prefers_absolute_pointer_when_axes_are_present() {
        let bus = MockBus::absolute_pointer(1279, 799);
        let dev = VirtioInputMmio::new(bus);
        assert_eq!(dev.detect_role(), Ok(DeviceRole::AbsolutePointer));
        assert_eq!(dev.read_absolute_axis_info(0), Ok(AbsoluteAxisInfo::new(0, 1279)));
        assert_eq!(dev.read_absolute_axis_info(1), Ok(AbsoluteAxisInfo::new(0, 799)));
    }

    #[test]
    fn detect_role_defaults_keyboard_when_optional_event_bitmaps_are_absent() {
        let bus = MockBus::keyboard_without_optional_configs();
        let dev = VirtioInputMmio::new(bus);
        assert_eq!(dev.detect_role(), Ok(DeviceRole::Keyboard));
    }

    #[test]
    fn raw_input_event_decodes_little_endian_layout() {
        let event = RawInputEvent::from_le_bytes([0x03, 0x00, 0x01, 0x00, 0x40, 0x01, 0x00, 0x00]);
        assert_eq!(event.kind(), InputEventKind::Absolute);
        assert_eq!(event.code(), 1);
        assert_eq!(event.value(), 320);
    }
}
