# DSoftBus-lite overview

DSoftBus-lite provides the userland-only distributed fabric used by Open Nexus
OS services. It focuses on host-first development: all transports are
implemented in Rust, and avoid any kernel dependencies.

**Hybrid contract (aligns with VISION + RFC-0005):**

- **Control plane (target)**: typed IDL frames (Cap'n Proto) for stable, evolvable service protocols.
- **Bring-up allowance**: early OS milestones may use compact, versioned **byte-frame** protocols over the
  same authenticated stream to keep dependencies and debugging surface small.
- **Data plane**: large payloads stay out-of-band (VMO/filebuffer style). Over DSoftBus this starts as
  chunked transfers with explicit bounds; later it can map to real VMOs without copying.

The daemon is responsible for three major tasks:

This is aligned with the project vision: distributed behavior is layered in userland (`softbusd`
later), while the kernel stays minimal and capability-based (see `docs/agents/VISION.md`).

1. **Discovery** – each node announces its device identifier, published
   services, and listening port. The host backend uses an in-process registry so
   tests can run without sockets; the OS build uses multicast UDP discovery via
   `netstackd`.
2. **Authenticated session establishment** – peers authenticate using Noise XK
   handshakes seeded with static keys derived from the identity daemon. During
   the handshake both sides sign an attestation covering their Noise static
   keys, providing proof of possession of the Ed25519 device key material.
3. **Reliable stream framing** – once the Noise transport is ready a framed
   stream carries Cap'n Proto request/response traffic. Each frame identifies a
   logical channel, letting the daemon multiplex service protocols such as
   `samgr` and `bundlemgr` over a single encrypted TCP connection.

Large payloads (bundle artifacts, images, etc.) remain out-of-band. On the host
we emulate kernel VMO handles by sending the bytes over a dedicated channel and
stashing them in the bundle manager's artifact store before the install request
is forwarded. Kernel integration will eventually map these handles to real VMOs
and avoid copying.

## Host vs OS split

The `userspace/dsoftbus` crate exposes the high level traits used by the daemon.
Two runtime tracks are relevant today:

- `cfg(nexus_env = "host")` implements discovery, Noise handshakes, framed streams,
  and host-first QUIC selection/proof surfaces:
  - An in-process transport for socketless tests (`InProcAuthenticator`).
  - A sockets-facade-backed transport (`FacadeAuthenticator`) layered over
    `userspace/nexus-net` (using `FakeNet` in tests).
  - Host QUIC v1 transport-selection/runtime path (`auto|tcp|quic`) with strict fail-closed
    semantics and deterministic fallback markers (TASK-0021 / RFC-0035 contract).
  - The UDP discovery announce payload has a versioned, bounded byte layout with golden vectors
    (`userspace/dsoftbus/src/discovery_packet.rs`, `userspace/dsoftbus/tests/discovery_packet.rs`).
  The discovery registry is process-local so multiple nodes can run inside a
  single integration test.
- OS runtime remains split by boundary:
  - `userspace/dsoftbus` `cfg(nexus_env = "os")` backend now exposes an explicit adapter seam
    (`BorrowedFrameTransport`) with deterministic `Unsupported` behavior for unimplemented paths.
  - TASK-0022 closure introduced a dedicated no_std core crate
    (`userspace/dsoftbus/core`, package `dsoftbus-core`) using `core + alloc` types for:
    - bounded correlation nonce guards,
    - payload-vs-channel identity enforcement,
    - bounded record rejects,
    - borrow-view/owned-record transport adapter boundaries.
  - This remains intentionally **hybrid-phased**:
    - phase-1 in TASK-0022: borrow-view-first core seam,
    - handle-first (VMO/filebuffer) canonical bulk path: follow-up scope only.
  - `source/services/dsoftbusd` OS daemon path remains the authority for current OS transport behavior:
  - Networking: virtio-net + smoltcp + IPC sockets facade (`netstackd`)
  - UDP discovery announce/receive (loopback scope) via `nexus-discovery-packet` + `nexus-peer-lru`
  - Noise XK handshake (`nexus-noise-xk` library)
  - TCP sessions over the sockets facade
  - See `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md` (Done)
  - QUIC on OS is disabled-by-default in TASK-0021 and remains follow-on scoped (`TASK-0023`).

Address-profile contracts used by these paths are documented in
`docs/architecture/network-address-matrix.md`.

## Current OS Implementation Status (2026-04-15)

| Feature | Status | Task |
|---------|--------|------|
| Networking (virtio-net + smoltcp) | ✅ Done | TASK-0003 |
| Noise XK handshake | ✅ Done | TASK-0003B |
| UDP discovery (loopback) | ✅ Done | TASK-0003C |
| Discovery-driven TCP connect | ✅ Done | TASK-0004 |
| Identity binding enforcement | ✅ Done | TASK-0004 |
| Dual-node proof | ✅ Done | TASK-0004 |
| Cross-VM sessions (2× QEMU) | ✅ Done (opt-in) | TASK-0005 |
| Remote proxy (`samgrd`/`bundlemgrd`, deny-by-default) | ✅ Done (opt-in) | TASK-0005 |
| Remote packagefs RO (`STAT/OPEN/READ/CLOSE`, authenticated streams) | ✅ Done | TASK-0016 |
| Daemon modular structure (`src/os/**`, thin `main.rs`) | ✅ Done | TASK-0015 |
| Host seam + security-negative tests (`p0_unit`, `reject_transport_validation`, `session_steps`) | ✅ Done | TASK-0015 |
| Host QUIC selection + real host QUIC transport proof | ✅ Done | TASK-0021 |
| OS QUIC default state | ✅ Disabled-by-default (explicit fallback) | TASK-0021/TASK-0023 boundary |
| no_std core seam (`dsoftbus-core` crate + transport-neutral contract helpers) | 🟨 In Review | TASK-0022 |

**2-VM proof harness (opt-in)**:
- Canonical harness: `tools/os2vm.sh`
- Deterministic non-DHCP networking: `netstackd` falls back to a MAC-derived static IPv4 under the 2-VM socket/mcast backend
  (`10.42.0.x/24`; node role mapping uses `10.42.0.10`/`10.42.0.11`, see matrix doc).

**RFC Contracts**:
- RFC-0007: DSoftBus OS Transport v1 (UDP discovery + TCP sessions)
- RFC-0008: DSoftBus Noise XK v1 (handshake + identity binding)
- RFC-0009: no_std Dependency Hygiene v1 (OS build policy)
- RFC-0028: Remote packagefs RO v1 (bounded `pkg:/` and `/packages/` access over authenticated streams)

By keeping the kernel unaware of IDL parsing or Cap'n Proto framing we preserve
its minimal trusted computing base. Only the userland daemon deals with schema
serialization and policy decisions.

## IPC Robustness (TASK-0008)

During TASK-0008 implementation, the following IPC patterns were established:

### Reply Correlation via Nonces

`dsoftbusd` ↔ `netstackd` RPCs include a trailing `u64` nonce for reply correlation:
- Prevents reply misassociation when multiple RPC calls are in-flight
- Receiver validates that response nonce matches request nonce
- Backward-compatible: requests without nonce receive responses without nonce

### Deterministic Slot Assignment

Core services use deterministic IPC slots assigned by `init-lite`:
- `netstackd` server: slots 5/6 (recv/send)
- `dsoftbusd` reply inbox: slots 5/6

Services should prefer `KernelClient::new_with_slots()` over routing queries during early bring-up.

### Capability Closure

All `CAP_MOVE` operations must explicitly close the reply capability on all exit paths (success, error, timeout) to prevent capability leaks and heap exhaustion.
