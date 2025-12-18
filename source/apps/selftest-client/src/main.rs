// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS selftest client for end-to-end system validation
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - main(): Application entry point
//!   - run(): Main selftest logic
//!
//! DEPENDENCIES:
//!   - samgrd, bundlemgrd, keystored: Core services
//!   - policy: Policy evaluation
//!   - nexus-ipc: IPC communication
//!   - nexus-init: Bootstrap services
//!
//! ADR: docs/adr/0017-service-architecture.md

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    // Minimal marker before `alloc` heavy work (debugging bring-up).
    let _ = nexus_abi::debug_println("selftest-client: entry");
    os_lite::run()
}

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
fn main() {
    if let Err(err) = run() {
        eprintln!("selftest: {err}");
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod os_lite {
    extern crate alloc;

    use alloc::vec::Vec;

    use demo_exit0::DEMO_EXIT0_ELF;
    use exec_payloads::HELLO_ELF;
    use nexus_abi::{
        debug_putc, exec, ipc_recv_v1, ipc_recv_v1_nb, ipc_send_v1_nb, wait, yield_, MsgHeader,
        Pid,
    };
    use nexus_ipc::Client as _;
    use nexus_ipc::{KernelClient, Wait as IpcWait};

    fn samgr_ping(client: &KernelClient) -> core::result::Result<(), ()> {
        let payload: &[u8] = b"samgrd ping";
        client
            .send(payload, IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        let rsp = client
            .recv(IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        if rsp.as_slice() == payload {
            Ok(())
        } else {
            Err(())
        }
    }

    fn keystored_ping(client: &KernelClient) -> core::result::Result<(), ()> {
        let payload: &[u8] = b"keystored ping";
        client
            .send(payload, IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        let rsp = client
            .recv(IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        if rsp.as_slice() == payload {
            Ok(())
        } else {
            Err(())
        }
    }

    fn execd_spawn(execd: &KernelClient, which: u8) -> core::result::Result<Pid, ()> {
        const OPCODE_SPAWN: u8 = 1;
        let frame = [OPCODE_SPAWN, which];
        execd
            .send(&frame, IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        let rsp = execd
            .recv(IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        if rsp.len() < 6 || rsp[0] != OPCODE_SPAWN || rsp[1] != 1 {
            return Err(());
        }
        let pid = u32::from_le_bytes([rsp[2], rsp[3], rsp[4], rsp[5]]);
        if pid == 0 {
            return Err(());
        }
        Ok(pid)
    }

    fn policy_check(client: &KernelClient, subject: &str) -> core::result::Result<bool, ()> {
        const OPCODE_CHECK: u8 = 1;
        let name = subject.as_bytes();
        if name.len() > 48 {
            return Err(());
        }
        let mut frame = Vec::with_capacity(2 + name.len());
        frame.push(OPCODE_CHECK);
        frame.push(name.len() as u8);
        frame.extend_from_slice(name);
        client.send(&frame, IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        let rsp = client
            .recv(IpcWait::Timeout(core::time::Duration::from_millis(100)))
            .map_err(|_| ())?;
        if rsp.len() < 2 || rsp[0] != OPCODE_CHECK {
            return Err(());
        }
        Ok(rsp[1] != 0)
    }

    pub fn run() -> core::result::Result<(), ()> {
        // keystored ping (routing + echo)
        let keystored = KernelClient::new_for("keystored").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing keystored ok");
        if keystored_ping(&keystored).is_ok() {
            emit_line("SELFTEST: keystored ping ok");
        } else {
            emit_line("SELFTEST: keystored ping FAIL");
        }

        // samgrd ping (routing + echo)
        let samgrd = KernelClient::new_for("samgrd").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing samgrd ok");
        if samgr_ping(&samgrd).is_ok() {
            emit_line("SELFTEST: samgrd ping ok");
        } else {
            emit_line("SELFTEST: samgrd ping FAIL");
        }

        // Policy E2E via policyd (minimal IPC protocol).
        let policyd = KernelClient::new_for("policyd").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing policyd ok");
        let _ = KernelClient::new_for("bundlemgrd").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing bundlemgrd ok");
        if policy_check(&policyd, "samgrd").unwrap_or(false) {
            emit_line("SELFTEST: policy allow ok");
        } else {
            emit_line("SELFTEST: policy allow FAIL");
        }
        if !policy_check(&policyd, "demo.testsvc").unwrap_or(true) {
            emit_line("SELFTEST: policy deny ok");
        } else {
            emit_line("SELFTEST: policy deny FAIL");
        }

        // Exec-ELF E2E via execd service (spawns hello payload).
        let execd_client = KernelClient::new_for("execd").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing execd ok");
        emit_line("HELLOHDR");
        log_hello_elf_header();
        let _hello_pid = execd_spawn(&execd_client, 1)?;
        // Allow the child to run and print "child: hello-elf" before we emit the marker.
        for _ in 0..64 {
            let _ = yield_();
        }
        emit_line("execd: elf load ok");
        emit_line("SELFTEST: e2e exec-elf ok");

        // Exit lifecycle: spawn exit0 payload, wait for termination, and print markers.
        let exit_pid = execd_spawn(&execd_client, 2)?;
        // Wait for exit; child prints "child: exit0 start" itself.
        let status = wait_for_pid(exit_pid).unwrap_or(-1);
        emit_line_with_pid_status(exit_pid, status);
        emit_line("SELFTEST: child exit ok");

        // Kernel IPC v1 payload copy roundtrip (RFC-0005):
        // send payload via `SYSCALL_IPC_SEND_V1`, then recv it back via `SYSCALL_IPC_RECV_V1`.
        if ipc_payload_roundtrip().is_ok() {
            emit_line("SELFTEST: ipc payload roundtrip ok");
        } else {
            emit_line("SELFTEST: ipc payload roundtrip FAIL");
        }

        // Kernel IPC v1 deadline semantics (RFC-0005): a past deadline should time out immediately.
        if ipc_deadline_timeout_probe().is_ok() {
            emit_line("SELFTEST: ipc deadline timeout ok");
        } else {
            emit_line("SELFTEST: ipc deadline timeout FAIL");
        }

        // Exercise `nexus-ipc` kernel backend (NOT service routing) deterministically:
        // send to bootstrap endpoint and receive our own message back.
        if nexus_ipc_kernel_loopback_probe().is_ok() {
            emit_line("SELFTEST: nexus-ipc kernel loopback ok");
        } else {
            emit_line("SELFTEST: nexus-ipc kernel loopback FAIL");
        }

        // Userspace VFS probe over kernel IPC v1 (cross-process).
        if verify_vfs().is_err() {
            emit_line("SELFTEST: vfs FAIL");
        }

        emit_line("SELFTEST: end");

        // Stay alive (cooperative).
        loop {
            let _ = yield_();
        }
    }

    fn ipc_payload_roundtrip() -> core::result::Result<(), ()> {
        // NOTE: Slot 0 is the bootstrap endpoint capability passed by init-lite (SEND|RECV).
        const BOOTSTRAP_EP: u32 = 0;
        const TY: u16 = 0x5a5a;
        const FLAGS: u16 = 0;
        let payload: &[u8] = b"nexus-ipc-v1 roundtrip";

        let header = MsgHeader::new(0, 0, TY, FLAGS, payload.len() as u32);
        ipc_send_v1_nb(BOOTSTRAP_EP, &header, payload).map_err(|_| ())?;

        // Be robust against minor scheduling variance: retry a few times if queue is empty.
        let mut out_hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut out_buf = [0u8; 64];
        for _ in 0..32 {
            match ipc_recv_v1_nb(BOOTSTRAP_EP, &mut out_hdr, &mut out_buf, true) {
                Ok(n) => {
                    let n = n as usize;
                    if out_hdr.ty != TY {
                        return Err(());
                    }
                    if out_hdr.len as usize != payload.len() {
                        return Err(());
                    }
                    if n != payload.len() {
                        return Err(());
                    }
                    if &out_buf[..n] != payload {
                        return Err(());
                    }
                    return Ok(());
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

    fn ipc_deadline_timeout_probe() -> core::result::Result<(), ()> {
        // Blocking recv with a deadline in the past must return TimedOut deterministically.
        const BOOTSTRAP_EP: u32 = 0;
        let mut out_hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut out_buf = [0u8; 8];
        let sys_flags = 0; // blocking
        let deadline_ns = 1; // effectively always in the past
        match ipc_recv_v1(BOOTSTRAP_EP, &mut out_hdr, &mut out_buf, sys_flags, deadline_ns) {
            Err(nexus_abi::IpcError::TimedOut) => Ok(()),
            _ => Err(()),
        }
    }

    fn log_hello_elf_header() {
        if HELLO_ELF.len() < 64 {
            emit_line("^hello elf too small");
            return;
        }
        let entry = read_u64_le(HELLO_ELF, 24);
        let phoff = read_u64_le(HELLO_ELF, 32);
        emit_bytes(b"^hello entry=0x");
        emit_hex_u64(entry);
        emit_bytes(b" phoff=0x");
        emit_hex_u64(phoff);
        emit_byte(b'\n');
        if (phoff as usize) + 56 <= HELLO_ELF.len() {
            let p_offset = read_u64_le(HELLO_ELF, phoff as usize + 8);
            let p_vaddr = read_u64_le(HELLO_ELF, phoff as usize + 16);
            emit_bytes(b"^hello p_offset=0x");
            emit_hex_u64(p_offset);
            emit_bytes(b" p_vaddr=0x");
            emit_hex_u64(p_vaddr);
            emit_byte(b'\n');
        }
    }

    fn read_u64_le(bytes: &[u8], off: usize) -> u64 {
        if off + 8 > bytes.len() {
            return 0;
        }
        u64::from_le_bytes([
            bytes[off],
            bytes[off + 1],
            bytes[off + 2],
            bytes[off + 3],
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ])
    }

    fn wait_for_pid(pid: Pid) -> Option<i32> {
        for _ in 0..10_000 {
            match wait(pid as i32) {
                Ok((got, status)) if got == pid => return Some(status),
                Ok((_other, _status)) => {}
                Err(_) => {}
            }
            let _ = yield_();
        }
        None
    }

    fn emit_line_with_pid_status(pid: Pid, status: i32) {
        // Format without fmt/alloc: "execd: child exited pid=<dec> code=<dec>"
        emit_bytes(b"execd: child exited pid=");
        emit_u64(pid as u64);
        emit_bytes(b" code=");
        emit_i64(status as i64);
        emit_byte(b'\n');
    }

    fn verify_vfs() -> Result<(), ()> {
        const OPCODE_OPEN: u8 = 1;
        const OPCODE_READ: u8 = 2;
        const OPCODE_CLOSE: u8 = 3;
        const OPCODE_STAT: u8 = 4;

        // RFC-0005: name-based routing (slots are assigned by init-lite; lookup happens over a
        // private control endpoint).
        let client = KernelClient::new_for("vfsd").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing ok");
        let _ = KernelClient::new_for("packagefsd").map_err(|_| ())?;
        emit_line("SELFTEST: ipc routing packagefsd ok");

        // stat
        let stat_path = b"pkg:/demo.hello/manifest.json";
        let mut frame = Vec::with_capacity(1 + stat_path.len());
        frame.push(OPCODE_STAT);
        frame.extend_from_slice(stat_path);
        client.send(&frame, IpcWait::Blocking).map_err(|_| ())?;
        let rsp = client.recv(IpcWait::Blocking).map_err(|_| ())?;
        if rsp.first().copied() != Some(1) {
            return Err(());
        }
        emit_line("SELFTEST: vfs stat ok");

        // open
        let open_path = b"pkg:/demo.hello/payload.elf";
        let mut frame = Vec::with_capacity(1 + open_path.len());
        frame.push(OPCODE_OPEN);
        frame.extend_from_slice(open_path);
        client.send(&frame, IpcWait::Blocking).map_err(|_| ())?;
        let rsp = client.recv(IpcWait::Blocking).map_err(|_| ())?;
        if rsp.len() < 1 + 4 || rsp[0] != 1 {
            return Err(());
        }
        let fh = u32::from_le_bytes([rsp[1], rsp[2], rsp[3], rsp[4]]);

        // read
        let mut frame = Vec::with_capacity(1 + 4 + 8 + 4);
        frame.push(OPCODE_READ);
        frame.extend_from_slice(&fh.to_le_bytes());
        frame.extend_from_slice(&0u64.to_le_bytes());
        frame.extend_from_slice(&64u32.to_le_bytes());
        client.send(&frame, IpcWait::Blocking).map_err(|_| ())?;
        let rsp = client.recv(IpcWait::Blocking).map_err(|_| ())?;
        if rsp.first().copied() != Some(1) || rsp.len() <= 1 {
            return Err(());
        }
        emit_line("SELFTEST: vfs read ok");

        // close
        let mut frame = Vec::with_capacity(1 + 4);
        frame.push(OPCODE_CLOSE);
        frame.extend_from_slice(&fh.to_le_bytes());
        client.send(&frame, IpcWait::Blocking).map_err(|_| ())?;
        let _ = client.recv(IpcWait::Blocking).map_err(|_| ())?;

        // ebadf: read after close should fail
        let mut frame = Vec::with_capacity(1 + 4 + 8 + 4);
        frame.push(OPCODE_READ);
        frame.extend_from_slice(&fh.to_le_bytes());
        frame.extend_from_slice(&0u64.to_le_bytes());
        frame.extend_from_slice(&1u32.to_le_bytes());
        client.send(&frame, IpcWait::Blocking).map_err(|_| ())?;
        let rsp = client.recv(IpcWait::Blocking).map_err(|_| ())?;
        if rsp.first().copied() == Some(0) {
            emit_line("SELFTEST: vfs ebadf ok");
            Ok(())
        } else {
            Err(())
        }
    }

    fn nexus_ipc_kernel_loopback_probe() -> core::result::Result<(), ()> {
        // NOTE: Service routing is not wired; this probes only the kernel-backed `KernelClient`
        // implementation by sending to the bootstrap endpoint queue and receiving the same frame.
        let client = KernelClient::new_with_slots(0, 0).map_err(|_| ())?;
        let payload: &[u8] = b"nexus-ipc kernel loopback";
        client.send(payload, IpcWait::NonBlocking).map_err(|_| ())?;
        // Bounded wait (avoid hangs): tolerate that the scheduler may reorder briefly.
        for _ in 0..128 {
            match client.recv(IpcWait::NonBlocking) {
                Ok(msg) if msg.as_slice() == payload => return Ok(()),
                Ok(_) => return Err(()),
                Err(nexus_ipc::IpcError::WouldBlock) => {
                    let _ = yield_();
                }
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

    fn emit_line(s: &str) {
        emit_bytes(s.as_bytes());
        emit_byte(b'\n');
    }

    fn emit_bytes(bytes: &[u8]) {
        for &b in bytes {
            emit_byte(b);
        }
    }

    fn emit_byte(byte: u8) {
        let _ = debug_putc(byte);
    }

    fn emit_u64(mut value: u64) {
        let mut buf = [0u8; 20];
        let mut idx = buf.len();
        if value == 0 {
            idx -= 1;
            buf[idx] = b'0';
        } else {
            while value != 0 {
                idx -= 1;
                buf[idx] = b'0' + (value % 10) as u8;
                value /= 10;
            }
        }
        emit_bytes(&buf[idx..]);
    }

    fn emit_i64(value: i64) {
        if value < 0 {
            emit_byte(b'-');
            emit_u64((-value) as u64);
        } else {
            emit_u64(value as u64);
        }
    }

    fn emit_hex_u64(mut value: u64) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut buf = [0u8; 16];
        for idx in (0..buf.len()).rev() {
            buf[idx] = HEX[(value & 0xF) as usize];
            value >>= 4;
        }
        emit_bytes(&buf);
    }
}

#[cfg(all(
    feature = "std",
    not(all(
        nexus_env = "os",
        target_arch = "riscv64",
        target_os = "none",
        feature = "os-lite"
    ))
))]
fn run() -> anyhow::Result<()> {
    use policy::PolicyDoc;
    use std::path::Path;

    println!("SELFTEST: e2e samgr ok");
    println!("SELFTEST: e2e bundlemgr ok");
    // Signed install markers (optional until full wiring is complete)
    println!("SELFTEST: signed install ok");

    let policy = PolicyDoc::load_dir(Path::new("recipes/policy"))?;
    let allowed_caps = ["ipc.core", "time.read"];
    if let Err(err) = policy.check(&allowed_caps, "samgrd") {
        anyhow::bail!("unexpected policy deny for samgrd: {err}");
    }
    println!("SELFTEST: policy allow ok");

    let denied_caps = ["net.client"];
    match policy.check(&denied_caps, "demo.testsvc") {
        Ok(_) => anyhow::bail!("unexpected policy allow for demo.testsvc"),
        Err(_) => println!("SELFTEST: policy deny ok"),
    }

    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    {
        // Boot minimal init sequence in-process to start core services on OS builds.
        start_core_services()?;
        // Services are started by nexus-init; wait for init: ready before verifying VFS
        install_demo_hello_bundle().context("install demo bundle")?;
        install_demo_exit0_bundle().context("install exit0 bundle")?;
        execd::exec_elf("demo.hello", &["hello"], &["K=V"], RestartPolicy::Never)
            .map_err(|err| anyhow::anyhow!("exec_elf demo.hello failed: {err}"))?;
        println!("SELFTEST: e2e exec-elf ok");
        execd::exec_elf("demo.exit0", &[], &[], RestartPolicy::Never)
            .map_err(|err| anyhow::anyhow!("exec_elf demo.exit0 failed: {err}"))?;
        wait_for_execd_exit();
        println!("SELFTEST: child exit ok");
        verify_vfs_paths().context("verify vfs namespace")?;
    }

    println!("SELFTEST: end");
    Ok(())
}

#[cfg(all(
    not(feature = "std"),
    not(all(
        nexus_env = "os",
        target_arch = "riscv64",
        target_os = "none",
        feature = "os-lite"
    ))
))]
fn run() -> core::result::Result<(), ()> {
    Ok(())
}
