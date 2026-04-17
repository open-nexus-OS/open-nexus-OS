extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::{yield_, MsgHeader, Pid};
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_i64, emit_line, emit_u64};

pub(crate) fn execd_spawn_image(
    execd: &KernelClient,
    requester: &str,
    image_id: u8,
) -> core::result::Result<Pid, ()> {
    // Execd IPC v1:
    // Request: [E, X, ver, op, image_id, stack_pages:u8, requester_len:u8, requester...]
    // Response: [E, X, ver, op|0x80, status:u8, pid:u32le]
    const MAGIC0: u8 = b'E';
    const MAGIC1: u8 = b'X';
    const VERSION: u8 = 1;
    const OP_EXEC_IMAGE: u8 = 1;
    const STATUS_OK: u8 = 0;
    const STATUS_DENIED: u8 = 4;

    let name = requester.as_bytes();
    if name.is_empty() || name.len() > 48 {
        return Err(());
    }
    let mut req = Vec::with_capacity(7 + name.len());
    req.push(MAGIC0);
    req.push(MAGIC1);
    req.push(VERSION);
    req.push(OP_EXEC_IMAGE);
    req.push(image_id);
    // Keep exec selftests bounded under the current kernel heap budget.
    req.push(4);
    req.push(name.len() as u8);
    req.extend_from_slice(name);
    let (send_slot, recv_slot) = execd.slots();
    let hdr = MsgHeader::new(0, 0, 0, 0, req.len() as u32);
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(2_000_000_000); // 2s
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        emit_line("SELFTEST: execd spawn send timeout");
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => {
                emit_line("SELFTEST: execd spawn send fail");
                return Err(());
            }
        }
        i = i.wrapping_add(1);
    }
    // Give execd a chance to run immediately after enqueueing (cooperative scheduler).
    let _ = yield_();
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                emit_line("SELFTEST: execd spawn timeout");
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n != 9 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
                    continue;
                }
                if buf[3] != (OP_EXEC_IMAGE | 0x80) {
                    continue;
                }
                return if buf[4] == STATUS_OK {
                    let pid = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                    if pid == 0 {
                        Err(())
                    } else {
                        Ok(pid)
                    }
                } else if buf[4] == STATUS_DENIED {
                    emit_line("SELFTEST: execd spawn denied");
                    Err(())
                } else {
                    emit_bytes(b"SELFTEST: execd spawn status 0x");
                    emit_hex_u64(buf[4] as u64);
                    emit_byte(b'\n');
                    Err(())
                };
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    }
}

pub(crate) fn execd_spawn_image_raw_requester(
    execd: &KernelClient,
    requester: &str,
    image_id: u8,
) -> core::result::Result<Vec<u8>, ()> {
    // Execd IPC v1:
    // Request: [E, X, ver, op, image_id, stack_pages:u8, requester_len:u8, requester...]
    const MAGIC0: u8 = b'E';
    const MAGIC1: u8 = b'X';
    const VERSION: u8 = 1;
    const OP_EXEC_IMAGE: u8 = 1;
    let name = requester.as_bytes();
    if name.is_empty() || name.len() > 48 {
        return Err(());
    }
    let mut req = Vec::with_capacity(7 + name.len());
    req.push(MAGIC0);
    req.push(MAGIC1);
    req.push(VERSION);
    req.push(OP_EXEC_IMAGE);
    req.push(image_id);
    req.push(4);
    req.push(name.len() as u8);
    req.extend_from_slice(name);
    execd.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(100))).map_err(|_| ())?;
    execd.recv(IpcWait::Timeout(core::time::Duration::from_millis(100))).map_err(|_| ())
}

pub(crate) fn execd_report_exit_with_dump_status(
    execd: &KernelClient,
    pid: Pid,
    code: i32,
    build_id: &str,
    dump_path: &str,
    dump_bytes: &[u8],
) -> core::result::Result<u8, ()> {
    const MAGIC0: u8 = b'E';
    const MAGIC1: u8 = b'X';
    const VERSION: u8 = 1;
    const OP_REPORT_EXIT: u8 = 2;
    if build_id.is_empty()
        || build_id.len() > 64
        || dump_path.is_empty()
        || dump_path.len() > 255
        || dump_bytes.is_empty()
        || dump_bytes.len() > 4096
    {
        return Err(());
    }

    let mut req = Vec::with_capacity(17 + build_id.len() + dump_path.len() + dump_bytes.len());
    req.push(MAGIC0);
    req.push(MAGIC1);
    req.push(VERSION);
    req.push(OP_REPORT_EXIT);
    req.extend_from_slice(&(pid as u32).to_le_bytes());
    req.extend_from_slice(&code.to_le_bytes());
    req.push(build_id.len() as u8);
    req.extend_from_slice(&(dump_path.len() as u16).to_le_bytes());
    req.extend_from_slice(&(dump_bytes.len() as u16).to_le_bytes());
    req.extend_from_slice(build_id.as_bytes());
    req.extend_from_slice(dump_path.as_bytes());
    req.extend_from_slice(dump_bytes);

    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, execd, &req, core::time::Duration::from_millis(500))
        .map_err(|_| ())?;
    let rsp =
        nexus_ipc::budget::recv_budgeted(&clock, execd, core::time::Duration::from_millis(500))
            .map_err(|_| ())?;
    if rsp.len() != 9 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
        return Err(());
    }
    if rsp[3] != (OP_REPORT_EXIT | 0x80) {
        return Err(());
    }
    Ok(rsp[4])
}

pub(crate) fn execd_report_exit_with_dump_status_legacy(
    execd: &KernelClient,
    pid: Pid,
    code: i32,
    build_id: &str,
    dump_path: &str,
) -> core::result::Result<u8, ()> {
    const MAGIC0: u8 = b'E';
    const MAGIC1: u8 = b'X';
    const VERSION: u8 = 1;
    const OP_REPORT_EXIT: u8 = 2;
    if build_id.is_empty() || build_id.len() > 64 || dump_path.is_empty() || dump_path.len() > 255 {
        return Err(());
    }
    let mut req = Vec::with_capacity(15 + build_id.len() + dump_path.len());
    req.push(MAGIC0);
    req.push(MAGIC1);
    req.push(VERSION);
    req.push(OP_REPORT_EXIT);
    req.extend_from_slice(&(pid as u32).to_le_bytes());
    req.extend_from_slice(&code.to_le_bytes());
    req.push(build_id.len() as u8);
    req.extend_from_slice(&(dump_path.len() as u16).to_le_bytes());
    req.extend_from_slice(build_id.as_bytes());
    req.extend_from_slice(dump_path.as_bytes());

    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, execd, &req, core::time::Duration::from_millis(500))
        .map_err(|_| ())?;
    let rsp =
        nexus_ipc::budget::recv_budgeted(&clock, execd, core::time::Duration::from_millis(500))
            .map_err(|_| ())?;
    if rsp.len() != 9 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
        return Err(());
    }
    if rsp[3] != (OP_REPORT_EXIT | 0x80) {
        return Err(());
    }
    Ok(rsp[4])
}

pub(crate) fn execd_report_exit_with_dump(
    execd: &KernelClient,
    pid: Pid,
    code: i32,
    build_id: &str,
    dump_path: &str,
    dump_bytes: &[u8],
) -> core::result::Result<(), ()> {
    const STATUS_OK: u8 = 0;
    let status =
        execd_report_exit_with_dump_status(execd, pid, code, build_id, dump_path, dump_bytes)?;
    if status != STATUS_OK {
        return Err(());
    }
    Ok(())
}

pub(crate) fn wait_for_pid(execd: &KernelClient, pid: Pid) -> Option<i32> {
    // Execd IPC v1:
    // Wait:     [E, X, ver, OP_WAIT_PID=3, pid:u32le]
    // Response: [E, X, ver, OP_WAIT_PID|0x80, status:u8, pid:u32le, code:i32le]
    const MAGIC0: u8 = b'E';
    const MAGIC1: u8 = b'X';
    const VERSION: u8 = 1;
    const OP_WAIT_PID: u8 = 3;
    const STATUS_OK: u8 = 0;

    let mut req = [0u8; 8];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_WAIT_PID;
    req[4..8].copy_from_slice(&(pid as u32).to_le_bytes());

    // Bounded retries to avoid hangs if execd is unavailable.
    let clock = nexus_ipc::budget::OsClock;
    for _ in 0..128 {
        if nexus_ipc::budget::send_budgeted(
            &clock,
            execd,
            &req,
            core::time::Duration::from_millis(200),
        )
        .is_err()
        {
            let _ = yield_();
            continue;
        }
        let rsp = match nexus_ipc::budget::recv_budgeted(
            &clock,
            execd,
            core::time::Duration::from_millis(500),
        ) {
            Ok(rsp) => rsp,
            Err(_) => {
                let _ = yield_();
                continue;
            }
        };
        if rsp.len() != 13 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
            return None;
        }
        if rsp[3] != (OP_WAIT_PID | 0x80) {
            return None;
        }
        if rsp[4] != STATUS_OK {
            return None;
        }
        let got = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]) as Pid;
        if got != pid {
            return None;
        }
        let code = i32::from_le_bytes([rsp[9], rsp[10], rsp[11], rsp[12]]);
        return Some(code);
    }
    None
}

pub(crate) fn emit_line_with_pid_status(pid: Pid, status: i32) {
    // Format without fmt/alloc: "execd: child exited pid=<dec> code=<dec>"
    emit_bytes(b"execd: child exited pid=");
    emit_u64(pid as u64);
    emit_bytes(b" code=");
    emit_i64(status as i64);
    emit_byte(b'\n');
}
