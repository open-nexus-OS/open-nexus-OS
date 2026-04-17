extern crate alloc;

use nexus_ipc::{Client, Wait as IpcWait};

use super::super::super::ipc::clients::cached_dsoftbusd_client;
use super::REMOTE_DSOFTBUS_WAIT_MS;
use crate::markers::emit_line;

pub(crate) fn dsoftbusd_remote_resolve(name: &str) -> core::result::Result<(), ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 1;
    const STATUS_OK: u8 = 0;

    // Bounded debug: if routing is missing, remote proof can never succeed.
    static mut ROUTE_LOGGED: bool = false;
    let d = match cached_dsoftbusd_client() {
        Ok(x) => x,
        Err(_) => {
            unsafe {
                if !ROUTE_LOGGED {
                    ROUTE_LOGGED = true;
                    emit_line("selftest-client: route dsoftbusd FAIL");
                }
            }
            return Err(());
        }
    };
    let n = name.as_bytes();
    if n.is_empty() || n.len() > 48 {
        return Err(());
    }
    let mut req = alloc::vec::Vec::with_capacity(5 + n.len());
    req.push(D0);
    req.push(D1);
    req.push(VER);
    req.push(OP);
    req.push(n.len() as u8);
    req.extend_from_slice(n);
    if d.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .is_err()
    {
        return Err(());
    }
    let rsp = d
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    if rsp.len() != 5 || rsp[0] != D0 || rsp[1] != D1 || rsp[2] != VER || rsp[3] != (OP | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }
    Ok(())
}

pub(crate) fn dsoftbusd_remote_bundle_list() -> core::result::Result<u16, ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 2;
    const STATUS_OK: u8 = 0;

    let d = cached_dsoftbusd_client().map_err(|_| ())?;
    let req = [D0, D1, VER, OP];
    d.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    let rsp = d
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    if rsp.len() != 7 || rsp[0] != D0 || rsp[1] != D1 || rsp[2] != VER || rsp[3] != (OP | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }
    Ok(u16::from_le_bytes([rsp[5], rsp[6]]))
}
