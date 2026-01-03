# RFC-0006: Userspace Networking v1 (virtio-net + smoltcp + sockets facade)

- Status: Draft (seed)
- Owners: @runtime
- Created: 2026-01-01
- Last Updated: 2026-01-02
- Links:
  - Tasks (execution + proof): `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - Device/MMIO gatekeeper: `tasks/TASK-0010-device-mmio-access-model.md`
  - Driver track alignment: `tasks/TRACK-NETWORKING-DRIVERS.md`
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (extension policy + identity rules)

## Status at a Glance

- **Phase 0 (Host-first logic + contract seed)**: ✅
  - **Done**:
    - `userspace/nexus-net` exists as the v1 contract seed (UDP/TCP facade traits + bounded constants + error model).
    - Deterministic host backend exists: `nexus_net::fake::FakeNet` (shared clock + deadline semantics; no external sockets).
    - Contract is exercised by higher layers (DSoftBus) deterministically via facade transport tests.
  - **Proof gate (host)**:
    - `cd /home/jenning/open-nexus-OS && just test-host`
- **Phase 1 (OS/QEMU: virtio-net + smoltcp + sockets facade)**: 🟨 (**in progress; enabled by `TASK-0010` proof path**)
  - **Done**:
    - OS backend crate exists: `userspace/nexus-net-os` (smoltcp + virtio-net) implementing the `userspace/nexus-net` facade traits.
    - QEMU virtio-net RX/TX is exercised end-to-end by userspace selftest (ARP + ICMP echo).
    - Marker proof exists and is gated in `scripts/qemu-test.sh`:
      - `net: virtio-net up`
      - `net: smoltcp iface up 10.0.2.15`
      - `SELFTEST: net ping ok`
      - `SELFTEST: net udp dns ok` (UDP send+recv proof via QEMU usernet DNS)
      - `SELFTEST: net tcp listen ok` (TCP facade smoke: bind/listen succeeds)
    - Networking ownership slice exists (no duplicate authority):
      - `netstackd` is included in os-lite boot and owns virtio-net + smoltcp.
      - Other services access networking via a narrow IPC facade (v0) instead of direct MMIO.
      - Marker proof exists and is gated in `scripts/qemu-test.sh`:
        - `netstackd: ready`
        - `netstackd: facade up`
  - **Next**:
    - TCP on-wire connect proof (needs a deterministic peer outside the VM; not loopback).
    - Service integration proofs (e.g. `dsoftbusd` OS backend; see RFC-0007 + `TASK-0003` Track B).
  - **Proof gate (OS/QEMU)**:
    - `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s just test-os`
- **Phase 2 (Follow-ups / hardening)**: ⬜
  - **Next**:
    - Expand protocol surface only via tasks (e.g. DHCP/DNS/ICMP) with explicit bounds + proof.
    - Additional hardening: fuzz/property tests, perf instrumentation, stress/backpressure regression.

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - A minimal, bounded **userspace networking stack shape**: virtio-net → smoltcp → a small sockets facade usable by OS services.
  - The **normative sockets facade surface** (what methods exist, what errors mean, what is bounded).
  - The **polling + timer model** that makes behavior deterministic and compatible with `no_std`-style event loops.
- **This RFC does NOT own**:
  - A kernel networking stack or kernel sockets.
  - A full POSIX sockets API (no `select/epoll`, no file descriptors, no “everything is a socket”).
  - Multi-VM / distributed system behavior (that is DSoftBus transport scope; see RFC-0007).
  - Any kernel/device access changes needed to make userspace virtio drivers possible (that is `TASK-0010`).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC MUST link to the task(s) that implement and prove each phase/milestone.
- For this RFC, execution truth is `TASK-0003` (host tests + OS/QEMU markers). `TASK-0010` gates Phase 1 OS/QEMU work.
- See `TASK-0003` section **“Stop conditions (Definition of Done)”** for the canonical proof gates.

## Context

`TASK-0003` defines “Networking step 1” as an OS userspace milestone:

- userspace virtio-net frontend,
- smoltcp integration (static IPv4 initially),
- a tiny sockets facade for other services,
- and a DSoftBus OS backend that uses UDP discovery + TCP sessions (RFC‑0007).

Today, true userspace virtio-net on QEMU `virt` requires safe MMIO mapping, which is **blocked/gated** on `TASK-0010`. We can still progress deterministically on host by locking down the sockets facade contract and using fake/in-memory transports for protocol-level tests.

## Goals

- Provide a **minimal** UDP/TCP surface for OS services that does not expose smoltcp types.
- Make the API **bounded** (no unbounded buffering, explicit size limits).
- Make the runtime model **deterministic** (polling + timers, no background threads required).
- Be explicit about what is gated on `TASK-0010` vs host-first.

## Non-Goals

- POSIX sockets compatibility.
- Kernel parsers/crypto or kernel “net stack”.
- DHCP/DNS/mDNS (follow-up tasks).
- High-performance / zero-copy (follow-up tasks).

## Constraints / invariants (hard requirements)

- **Tasks are execution truth**: stop conditions + proof commands live in `TASK-0003`.
- **Determinism**: no nondeterministic background behavior; all progression is driven by explicit `poll(now)` calls and explicit timers.
- **No fake success**: OS/QEMU markers must not claim “up/ok” unless the real behavior happened (per `TASK-0003`).
- **Bounded resources**:
  - RX/TX rings are bounded.
  - Socket buffers are bounded.
  - Event loops must avoid unbounded busy loops; polling must yield and respect deadlines.
- **Security floor**:
  - No kernel crypto/parsers for networking.
  - For security-critical protocols layered above networking, apply RFC‑0005 identity-binding rules where relevant.

## Proposed design

### Contract / interface (normative)

The sockets facade is a minimal abstraction meant to support `dsoftbusd` (RFC‑0007) and early OS services.
It is *not* POSIX.

Phase 0 implementation note:

- The Phase‑0 contract seed is codified in `userspace/nexus-net` (host-first), including:
  - sockets facade traits (`NetStack`, `UdpSocket`, `TcpListener`, `TcpStream`),
  - bounded buffer helpers,
  - a deterministic in-memory backend (`nexus_net::fake::FakeNet`) for tests (no external network dependency).
  - a DSoftBus host transport adapter exists that runs over this facade for deterministic proof (`userspace/dsoftbus` facade transport tests).
  - preferred regression proof remains `just test-host` (CI parity; see `docs/testing/index.md`).

#### Types

- `NetInstant`: monotonic tick (`u64`) used for `poll(now)` and absolute deadlines. Units are backend-defined; callers MUST use the same tick domain for both `poll` and deadlines.
- `NetIpAddrV4`: IPv4 only for v1 (`[u8; 4]`), later expandable by versioning.
- `NetSocketAddrV4`: `{ ip: NetIpAddrV4, port: u16 }`.

Bounds (v1, contract-level):

- `MAX_UDP_DATAGRAM_BYTES`
- `MAX_TCP_WRITE_BYTES`

#### Errors (normative)

All operations return deterministic errors. No operation silently blocks.

- `NetError::Unsupported`: feature not present in this build/backend.
- `NetError::WouldBlock`: operation would block; caller must retry after `poll`.
- `NetError::TimedOut`: deadline exceeded.
- `NetError::InvalidInput`: malformed addr/len/state.
- `NetError::AddrInUse`: bind conflict.
- `NetError::NotConnected`: TCP stream not connected.
- `NetError::Disconnected`: remote closed / reset observed.
- `NetError::NoBufs`: bounded resource exhausted (explicit backpressure).
- `NetError::Internal`: unexpected backend failure (must still be deterministic and non-panicking in OS services).

#### Polling + timer model (normative)

The facade is driven by an explicit cooperative loop:

- `poll(now: NetInstant)` advances the networking backend and processes RX/TX.
- `next_wake() -> Option<NetInstant>` returns an optional absolute wake tick that the runtime should use to schedule the next poll.

No background threads are required, and behavior must be reproducible under deterministic host tests.

#### Socket operations (normative)

Minimum v1 surface:

- UDP
  - `udp_bind(local: NetSocketAddrV4) -> UdpSocket`
  - `UdpSocket::send_to(buf: &[u8], remote: NetSocketAddrV4) -> Result<usize, NetError>`
  - `UdpSocket::recv_from(buf: &mut [u8]) -> Result<(usize, NetSocketAddrV4), NetError>`
- TCP
  - `tcp_connect(remote: NetSocketAddrV4, deadline: Option<NetInstant>) -> TcpStream`
  - `tcp_listen(local: NetSocketAddrV4, backlog: usize /* bounded */) -> TcpListener`
  - `TcpListener::accept(deadline: Option<NetInstant>) -> TcpStream`
  - `TcpStream::read(deadline: Option<NetInstant>, buf: &mut [u8]) -> Result<usize, NetError>`
  - `TcpStream::write(deadline: Option<NetInstant>, buf: &[u8]) -> Result<usize, NetError>`
  - `TcpStream::close()` (idempotent)

Notes:

- All buffers are caller-provided; internal buffering is bounded and explicitly documented.
- No implicit DNS.
- No implicit “blocking forever”: deadlines are explicit; `WouldBlock` is normal flow-control.

### Phases / milestones (contract-level)

- **Phase 0 (Host-first)**:
  - Lock down the sockets facade contract and error model.
  - Provide a deterministic host backend (fake/in-memory is acceptable) so higher layers (DSoftBus) can be proven without OS networking.
  - Proof is via `TASK-0003` host tests.
- **Phase 1 (OS/QEMU)**:
  - Implement userspace virtio-net + smoltcp + sockets facade on OS/QEMU.
  - This phase is **gated on `TASK-0010`** (safe MMIO mapping for virtio devices).
  - Proof is via `TASK-0003` QEMU marker suite.
- **Phase 2 (Follow-ups)**:
  - DHCP/ICMP/DNS (as separate tasks).
  - Hardening: rate limits, fuzz/property testing, negative cases, perf instrumentation.

## Security considerations

- **Threat model**: malformed frames, resource exhaustion, spoofed identity at higher layers.
- **Mitigations**:
  - bounded buffers and explicit backpressure (`NoBufs`),
  - deterministic polling and deadlines (no unbounded busy loops),
  - higher-level identity binding and authorization is handled above this facade; follow RFC‑0005 extension policy for any security-critical surface changes.
- **Open risks**:
  - Until `TASK-0010` is complete, OS/QEMU networking is blocked; we must not ship “fake” markers.

## Failure model (normative)

- `WouldBlock` and `NoBufs` are **normal** signals; callers must drive the poll loop and retry.
- No implicit retries that could hide liveness problems or introduce nondeterminism.
- Errors must be stable across backends (host vs OS) where meaningfully comparable; differences must be documented as `Unsupported`.
- Deadlines are **absolute** `NetInstant` ticks:
  - Backends MUST interpret `deadline: Option<NetInstant>` in the same tick domain as `poll(now)`.
  - If `now > deadline`, operations MUST return `TimedOut` deterministically (no partial progress).
  - Host-first backends and tests MUST NOT rely on wall-clock time for correctness; they may advance by calling `poll(now)` and retrying.

## Proof / validation strategy (required)

Tasks are execution truth; see `TASK-0003` “Stop conditions (Definition of Done)” for the canonical proof commands and required coverage.

### Proof (Host)

Preferred workflow (CI parity; runs full host suite):

```bash
cd /home/jenning/open-nexus-OS && just test-host
```

Narrow proof (TASK-0003 Track A focus):

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- --nocapture
```

### Proof (OS/QEMU) (gated on TASK-0010)

Preferred workflow (CI parity; wrapper around `scripts/qemu-test.sh`):

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s just test-os
```

Narrow proof (task harness; equivalent to `just test-os`):

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

## Alternatives considered

- Expose smoltcp types directly to services: rejected (leaks stack internals; hard to version/bound).
- “Just implement POSIX sockets”: rejected for scope and drift risk at this stage.

## Open questions

- What is the minimal timer/clock type that works across host + OS without pulling heavy deps?
- How do we express bounded backlog and buffer sizing defaults so they are consistent across services?

## Checklist (keep current)

- [x] Scope boundaries are explicit; cross-RFC ownership is linked.
- [x] Task(s) exist for each milestone and contain stop conditions + proof (`TASK-0003`; Phase 1 gated on `TASK-0010`).
- [x] Proof is “honest green” (tests/markers), not log-grep optimism (host: `cargo test -p dsoftbus`, preferred: `just test-host`).
- [x] Determinism + bounded resources are specified (poll/timer model + explicit bounds in `userspace/nexus-net`).
- [x] Security invariants are stated and have at least one regression proof (bounded buffers + deterministic errors; exercised by `nexus-net` + `dsoftbus` facade tests).
- [x] `nexus-net` contract seed is test-guarded (bounds + deadline semantics have regression tests).
- [ ] If claiming stable ABI/on-wire surfaces: golden vectors + layout/compat tests exist. (Not claimed for v1 facade yet.)
- [x] Stubs (if any) are explicitly labeled and non-authoritative (OS/QEMU is gated; no fake success markers).

### Phase 0 implementation checklist (host-first, contract seed)

- [x] Contract crate exists: `userspace/nexus-net`.
- [x] Facade API exists (`NetStack`/`UdpSocket`/`TcpListener`/`TcpStream`) and is versioned as “v1 contract seed”.
- [x] Deterministic test backend exists: `nexus_net::fake::FakeNet` (no external sockets).
- [x] Buffer bounds are enforced by helpers + tests (UDP/TCP write limits).
- [x] Deadlines are part of the contract surface and are exercised deterministically in `FakeNet` tests (timeout behavior).
- [x] Facade adapters do not require wall-clock sleeps for progress; host-only backends can advance via `poll(now)` ticks (deterministic).
- [x] Preferred CI-parity workflow is documented and green: `just test-host`.

### Phase 1 implementation checklist (OS/QEMU: sockets facade backend)

- [x] OS backend crate exists (kept separate from contract crate): `userspace/nexus-net-os`.
- [x] OS backend compiles for `riscv64imac-unknown-none-elf` with `no_std` + `alloc`.
- [x] OS backend uses virtio-net + smoltcp (no kernel net stack).
- [x] QEMU marker gate proves on-wire L2/L3 works (ARP + ICMP echo): `SELFTEST: net ping ok`.
- [x] QEMU marker gate proves UDP send+recv over usernet works (DNS): `SELFTEST: net udp dns ok`.
- [x] QEMU marker gate proves TCP bind/listen works (smoke): `SELFTEST: net tcp listen ok`.
- [x] IPC facade exposes a bounded local-only UDP loopback for deterministic service proofs (netstackd `LoopUdp` v1; used by `dsoftbusd` discovery).
- [ ] TCP on-wire connect proof exists (needs a deterministic peer, e.g. QEMU usernet `hostfwd` + a host echo service).
- [x] Services consume the facade via a stable ownership model (netstackd owns device; services use IPC facade) with explicit task + proof (`TASK-0003` Track B).
