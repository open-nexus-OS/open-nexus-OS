use net_virtio::{VirtioNetMmio, VIRTIO_DEVICE_ID_NET, VIRTIO_MMIO_MAGIC};

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line, emit_u64};

pub(crate) struct MmioBus {
    pub(crate) base: usize,
}

impl nexus_hal::Bus for MmioBus {
    fn read(&self, addr: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base + addr) as *const u32) }
    }
    fn write(&self, addr: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.base + addr) as *mut u32, value) }
    }
}

pub(crate) fn mmio_map_probe() -> core::result::Result<(), ()> {
    // Capability is distributed by init (policy-gated) for the virtio-net window.
    const MMIO_CAP_SLOT: u32 = 48;
    // Choose a VA in the same region already used by the exec_v2 stack/meta/info mappings to
    // avoid allocating additional page-table levels (keeps kernel heap usage bounded).
    const MMIO_VA: usize = 0x2000_e000;

    fn emit_mmio_err(stage: &str, err: nexus_abi::AbiError) {
        emit_bytes(b"SELFTEST: mmio ");
        emit_bytes(stage.as_bytes());
        emit_bytes(b" err=");
        // Stable enum-to-string mapping (no alloc).
        let s = match err {
            nexus_abi::AbiError::InvalidSyscall => "InvalidSyscall",
            nexus_abi::AbiError::CapabilityDenied => "CapabilityDenied",
            nexus_abi::AbiError::IpcFailure => "IpcFailure",
            nexus_abi::AbiError::SpawnFailed => "SpawnFailed",
            nexus_abi::AbiError::TransferFailed => "TransferFailed",
            nexus_abi::AbiError::ChildUnavailable => "ChildUnavailable",
            nexus_abi::AbiError::NoSuchPid => "NoSuchPid",
            nexus_abi::AbiError::InvalidArgument => "InvalidArgument",
            nexus_abi::AbiError::Unsupported => "Unsupported",
        };
        emit_bytes(s.as_bytes());
        emit_byte(b'\n');
    }

    // Step 1 (TASK-0010): prove we can map a MMIO window and read a known register.
    match nexus_abi::mmio_map(MMIO_CAP_SLOT, MMIO_VA, 0) {
        Ok(()) => {}
        Err(e) => {
            emit_mmio_err("map0", e);
            return Err(());
        }
    }
    let magic0 = unsafe { core::ptr::read_volatile((MMIO_VA + 0x000) as *const u32) };
    if magic0 != VIRTIO_MMIO_MAGIC {
        emit_bytes(b"SELFTEST: mmio magic0=0x");
        emit_hex_u64(magic0 as u64);
        emit_byte(b'\n');
        return Err(());
    }

    // Step 2 (TASK-0003 Track B seed): verify virtio-net device ID in the granted window.
    // This stays within the bounded per-device window (no slot scanning).
    let version = unsafe { core::ptr::read_volatile((MMIO_VA + 0x004) as *const u32) };
    let device_id = unsafe { core::ptr::read_volatile((MMIO_VA + 0x008) as *const u32) };
    let _vendor_id = unsafe { core::ptr::read_volatile((MMIO_VA + 0x00c) as *const u32) };
    if (version == 1 || version == 2) && device_id == VIRTIO_DEVICE_ID_NET {
        // TASK-0010 proof scope: MMIO map + safe register reads only.
        //
        // Networking ownership is moving to `netstackd` (TASK-0003 Track B), so this client
        // must NOT bring up virtio queues or smoltcp when netstackd is present.
        let dev = VirtioNetMmio::new(MmioBus { base: MMIO_VA });
        let info = match dev.probe() {
            Ok(info) => info,
            Err(_) => {
                emit_line("SELFTEST: virtio-net probe FAIL");
                return Err(());
            }
        };
        emit_bytes(b"SELFTEST: virtio-net mmio ver=");
        emit_u64(info.version as u64);
        emit_byte(b'\n');
    }

    // TASK-0010 proof remains: mapping + reading known register succeeded.
    Ok(())
}

pub(crate) fn cap_query_mmio_probe() -> core::result::Result<(), ()> {
    const MMIO_CAP_SLOT: u32 = 48;
    let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    nexus_abi::cap_query(MMIO_CAP_SLOT, &mut info).map_err(|_| ())?;
    // 2 = DeviceMmio
    if info.kind_tag != 2 || info.base == 0 || info.len == 0 {
        return Err(());
    }
    Ok(())
}

pub(crate) fn cap_query_vmo_probe() -> core::result::Result<(), ()> {
    // Allocate a small VMO and ensure we can query its physical window deterministically.
    let vmo = nexus_abi::vmo_create(4096).map_err(|_| ())?;
    let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    nexus_abi::cap_query(vmo, &mut info).map_err(|_| ())?;
    // 1 = VMO
    if info.kind_tag != 1 || info.base == 0 || info.len < 4096 {
        return Err(());
    }
    Ok(())
}
