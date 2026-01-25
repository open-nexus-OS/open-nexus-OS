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
   tests can run without sockets; the OS build will swap in a multicast-based
   discovery layer once networking is available.
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
Two backends exist today:

- `cfg(nexus_env = "host")` implements discovery, Noise handshakes, and framed
  streams with deterministic, host-first transports:
  - An in-process transport for socketless tests (`InProcAuthenticator`).
  - A sockets-facade-backed transport (`FacadeAuthenticator`) layered over
    `userspace/nexus-net` (using `FakeNet` in tests).
  - The UDP discovery announce payload has a versioned, bounded byte layout with golden vectors
    (`userspace/dsoftbus/src/discovery_packet.rs`, `userspace/dsoftbus/tests/discovery_packet.rs`).
  The discovery registry is process-local so multiple nodes can run inside a
  single integration test.
- `cfg(nexus_env = "os")` — **OS transport is now implemented** (as of 2026-01-07):
  - Networking: virtio-net + smoltcp + IPC sockets facade (`netstackd`)
  - UDP discovery announce/receive (loopback scope) via `nexus-discovery-packet` + `nexus-peer-lru`
  - Noise XK handshake (`nexus-noise-xk` library)
  - TCP sessions over the sockets facade
  - See `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md` (Done)

## Current OS Implementation Status (2026-01-25)

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

**2-VM proof harness (opt-in)**:
- Canonical harness: `tools/os2vm.sh`
- Deterministic non-DHCP networking: `netstackd` falls back to a MAC-derived static IPv4 under the 2-VM socket/mcast backend

**RFC Contracts**:
- RFC-0007: DSoftBus OS Transport v1 (UDP discovery + TCP sessions)
- RFC-0008: DSoftBus Noise XK v1 (handshake + identity binding)
- RFC-0009: no_std Dependency Hygiene v1 (OS build policy)

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
