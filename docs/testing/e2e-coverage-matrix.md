# E2E Test Coverage Matrix

This document maps E2E test coverage across host-first and OS-last layers for Open Nexus OS.

## Philosophy

- **Host E2E tests** (`tests/*_e2e/`): Fast, deterministic, in-process integration tests using loopback channels and `FakeNet`. Ideal for iteration and CI.
- **QEMU tests** (`scripts/qemu-test.sh`, `tools/os2vm.sh`): Real OS, real hardware (virtio), real network. Slower, opt-in for expensive scenarios (2-VM).

Both layers are **necessary and complementary**: host tests prove logic correctness, QEMU tests prove OS reality.

---

## Coverage Matrix

| Feature | Unit Tests | Host E2E | QEMU 1-VM | QEMU 2-VM (opt-in) |
| ------- | ---------- | -------- | --------- | ------------------ |
| **Service IPC (samgrd, bundlemgrd)** | ✅ | ✅ (`nexus-e2e`) | ✅ | ✅ |
| **Policy enforcement (policyd)** | ✅ | ✅ (`e2e_policy`) | ✅ | ❌ |
| **VFS (packagefsd, vfsd)** | ✅ | ✅ (`vfs-e2e`) | ✅ | ❌ |
| **DSoftBus discovery** | ✅ | ✅ (`remote_e2e`) | ✅ (loopback) | ✅ (cross-VM) |
| **Noise XK authentication** | ✅ | ✅ (`remote_e2e`) | ✅ | ✅ |
| **Remote proxy (samgrd/bundlemgrd)** | ❌ | ✅ (`remote_e2e`) | ❌ | ✅ |
| **Real network (virtio-net, smoltcp)** | ❌ | ❌ | ✅ | ✅ |
| **Logging (logd journal)** | ✅ (31 tests) | ✅ (`logd-e2e`, 7 tests) | ✅ | ❌ |
| **Crash reporting (execd → logd)** | ❌ | ✅ (`logd-e2e`) | ✅ | ❌ |
| **Multi-service concurrency** | ❌ | ✅ (`logd-e2e`) | ✅ | ❌ |

---

## Test Suites

### Host E2E (`tests/*_e2e/`)

| Suite | Tests | Focus | Command |
| ----- | ----- | ----- | ------- |
| `nexus-e2e` | 5 | samgrd/bundlemgrd/keystored integration, signature validation | `cargo test -p nexus-e2e` |
| `remote_e2e` | 1 | DSoftBus discovery, Noise XK, remote proxy | `cargo test -p remote_e2e` |
| `logd-e2e` | 7 | logd journal, overflow, crash reports, concurrency | `cargo test -p logd-e2e` |
| `vfs-e2e` | 1 | packagefsd/vfsd/bundlemgrd integration | `cargo test -p vfs-e2e` |
| `e2e_policy` | 1 | policyd allow/deny, capability checks | `cargo test -p e2e_policy` |
| **Total** | **15** | | `just test-e2e` |

### QEMU Tests

| Suite           | Scope                     | Command                       | Duration |
| --------------- | ------------------------- | ----------------------------- | -------- |
| `qemu-test.sh`  | 1-VM smoke (95+ markers)  | `just test-os`                | ~60s     |
| `os2vm.sh`      | 2-VM cross-VM DSoftBus    | `RUN_OS2VM=1 tools/os2vm.sh`  | ~180s    |

---

## When to Use Each Layer

### Use **Host E2E** when

- ✅ Testing service integration logic (IPC, protocol, state)
- ✅ Iterating on new features (fast feedback)
- ✅ Running in CI (deterministic, no QEMU overhead)
- ✅ Debugging complex flows (in-process, no UART logs)

### Use **QEMU 1-VM** when

- ✅ Verifying OS boot sequence (kernel → init → services)
- ✅ Testing real hardware (virtio-net, virtio-blk, UART)
- ✅ Proving end-to-end marker contracts (95+ markers)
- ✅ Validating syscall/IPC paths (kernel integration)

### Use **QEMU 2-VM** (opt-in) when

- ✅ Proving cross-VM networking (real UDP, multicast, L2)
- ✅ Testing distributed scenarios (DSoftBus sessions, remote proxy)
- ✅ Validating security (Noise XK over real network)
- ❌ **Not for**: Iteration (too slow), CI (opt-in only)

---

## Example: DSoftBus Testing Strategy

| Layer | What's Tested | Why |
| ----- | ------------- | --- |
| **Unit** (`userspace/dsoftbus/`) | Discovery protocol, Noise XK state machine, frame encoding | Fast, focused, no dependencies |
| **Host E2E** (`remote_e2e`) | Full stack: discovery → auth → remote proxy (in-process, FakeNet) | Deterministic, debuggable, CI-friendly |
| **QEMU 1-VM** (`qemu-test.sh`) | DSoftBus loopback (UDP, TCP), session establishment, markers | Proves OS integration (syscalls, virtio) |
| **QEMU 2-VM** (`os2vm.sh`) | Cross-VM discovery, real UDP multicast, remote proxy over network | Proves distributed reality (opt-in) |

---

## References

- **Testing methodology**: `docs/testing/index.md`
- **TASK-0005** (DSoftBus remote proxy): `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
- **TASK-0006** (logd journal): `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
- **RFC-0010** (DSoftBus remote proxy): `docs/rfcs/RFC-0010-dsoftbus-remote-proxy-v1.md`
- **RFC-0011** (logd journal): `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
