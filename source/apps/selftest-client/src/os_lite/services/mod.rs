pub(crate) mod bootctl;
pub(crate) mod bundlemgrd;
pub(crate) mod execd;
pub(crate) mod keystored;
pub(crate) mod logd;
pub(crate) mod metricsd;
pub(crate) mod policyd;
pub(crate) mod samgrd;
pub(crate) mod statefs;

use nexus_ipc::KernelClient;

pub(crate) fn core_service_probe(
    svc: &KernelClient,
    magic0: u8,
    magic1: u8,
    version: u8,
    op: u8,
) -> core::result::Result<(), ()> {
    let frame = [magic0, magic1, version, op];
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, svc, &frame, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, svc, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    if rsp.len() < 5 || rsp[0] != magic0 || rsp[1] != magic1 || rsp[2] != version {
        return Err(());
    }
    if rsp[3] != (op | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    Ok(())
}

pub(crate) fn core_service_probe_policyd(svc: &KernelClient) -> core::result::Result<(), ()> {
    // policyd expects frames to be at least 6 bytes (v1 response shape).
    let frame = [b'P', b'O', 1, 0x7f, 0, 0];
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, svc, &frame, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, svc, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    if rsp.len() < 6 || rsp[0] != b'P' || rsp[1] != b'O' || rsp[2] != 1 {
        return Err(());
    }
    if rsp[3] != (0x7f | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    Ok(())
}
