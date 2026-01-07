# RFC-0007: DSoftBus OS Transport v1 (UDP discovery + TCP sessions over sockets facade)

- Status: In Progress (Phase 0 ✅, Phase 1 loopback ✅ real Noise XK, Phase 1 full → TASK-0004)
- Owners: @runtime
- Created: 2026-01-01
- Last Updated: 2026-01-07 (real Noise XK handshake implemented)
- Links:
  - Tasks (execution + proof): `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - Follow-up Noise handshake: `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
  - ADR: `docs/adr/0005-dsoftbus-architecture.md`
  - Docs: `docs/distributed/dsoftbus-lite.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0006-userspace-networking-v1.md` (sockets facade)
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (extension policy + identity binding rules)
    - `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md` (Noise XK handshake + identity binding contract)

## Status at a Glance

- **Phase 0 (Host-first: deterministic socketless transport tests + honest OS stubs)**: ✅
  - **Done**:
    - Socketless deterministic auth/session proof exists: `userspace/dsoftbus/tests/host_transport.rs` (in-proc transport; no real sockets).
    - Sockets-facade transport proof exists over `userspace/nexus-net` (`FakeNet`): `userspace/dsoftbus/tests/facade_transport.rs`.
    - Discovery announce packet v1 is versioned + bounded + has golden vectors: `userspace/dsoftbus/{src,tests}/discovery_packet.rs`.
    - UDP discovery over the sockets facade is proven deterministically (cache-seeded watch + multi-announce): `userspace/dsoftbus/tests/facade_discovery.rs`.
    - Multi-node e2e harness runs over the facade contract (no flaky loopback sockets): `tests/remote_e2e` (shared `FakeNet`).
    - OS backend is “honest”: `userspace/dsoftbus/src/os.rs` returns `Unsupported` / deterministic errors (no `todo!()` panics).
  - **Proof gate (host)**:
    - `cd /home/jenning/open-nexus-OS && just test-host`
    - `cd /home/jenning/open-nexus-OS && just test-e2e`
- **Phase 1 (OS/QEMU: UDP discovery + TCP sessions)**: 🟨 (**in progress; networking ownership slice is now proven**)  
  - `dsoftbusd` now emits a bounded, versioned UDP announce/recv proof over the netstackd facade (loopback scope), performs a bounded challenge/response identity check (client pub + server pub + nonce-derived tag) with marker `dsoftbusd: auth ok`, and keeps the TCP session proof green in `just test-os`.
  - **Done (prerequisites)**:
    - OS/QEMU userspace networking marker gates exist (from RFC‑0006 / `TASK-0003`):
      - `net: smoltcp iface up 10.0.2.15`
      - `SELFTEST: net ping ok`
      - `SELFTEST: net udp dns ok`
      - `SELFTEST: net tcp listen ok`
  - **Done (Phase 1 so far)**:
    - `netstackd` owns virtio-net + smoltcp and exports a minimal IPC sockets facade (v0).
    - `dsoftbusd` is wired into os-lite init and proves:
      - **bounded UDP announce/recv v1 (loopback scope)** over the netstackd facade.
      - **bounded identity gate (pubkey + signature blob) + local-only TCP session** over the same facade.
    - Marker proof exists and is gated in `scripts/qemu-test.sh`:
      - `dsoftbusd: os transport up (udp+tcp)`
      - `dsoftbusd: auth ok`
      - `dsoftbusd: os session ok`
      - `SELFTEST: dsoftbus os connect ok`
      - `SELFTEST: dsoftbus ping ok`
  - **Next**:
    - (Done) Networking ownership/distribution for services:
      - `netstackd` owns MMIO + smoltcp and exports a minimal sockets IPC facade (v0).
      - `dsoftbusd` consumes the facade (no direct MMIO) and proves a local-only TCP session.
    - Implement the OS transport backend over the sockets facade (RFC‑0006): UDP announce/receive + TCP session connect/accept + Noise handshake reuse.
    - Emit the `TASK-0003` required markers for discovery/session/auth (no fake success).
  - **Proof gate (OS/QEMU)**:
    - `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s just test-os`
- **Phase 2 (Follow-ups)**: ⬜
  - **Next**:
    - Discovery hardening (rate limits, replay handling policy, negative-case stress) and additional on-wire versions (each with golden vectors).
    - **RPC Format Migration**: Migrate remote service calls from OS-lite byte frames to schema-based RPC (Cap'n Proto). See "RPC Format Migration Path" below.
- **Phase 3 (QUIC Transport)**: ⬜
  - Replace TCP + custom framing with QUIC (IETF RFC 9000) for transport.
  - Keep Noise XK for handshake crypto (not TLS 1.3).
  - See TASK-0021 for implementation.

### RPC Format Migration Path (Technical Debt)

**Current state (Phase 1 bring-up):**
- TASK-0005 remote proxy uses **OS-lite byte frames** (`SM`, `BN` magic)
- TASK-0016/0017 follow the same pattern (`PK`, statefs frames)
- This is a **conscious shortcut** to avoid schema dependencies during bring-up

**Target state (Phase 2+):**
- Migrate to **Cap'n Proto IDL** or equivalent stable schema
- Benefits: schema evolution, versioning, cross-language tooling
- Aligns with OpenHarmony DSoftBus IDL and Fuchsia FIDL patterns

**Migration trigger:**
- When TASK-0020 (Streams v2 Mux) lands — natural refactor point
- Or when TASK-0021 (QUIC) lands — new transport = new RPC layer

**Affected tasks:**
- TASK-0005: Remote proxy (samgrd/bundlemgrd)
- TASK-0016: Remote PackageFS
- TASK-0017: Remote StateFS
- Any new remote service

**Tracking:** Create dedicated RFC "DSoftBus RPC Schema v1" when migration begins.

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The OS-facing DSoftBus transport contract for the “local only” milestone:
    - UDP discovery (local subnet),
    - TCP sessions (loopback + local subnet),
    - Noise-based authentication and identity checks in userland.
  - The expectations DSoftBus places on the sockets facade (RFC‑0006) and OS runtime model (polling/timers).
- **This RFC does NOT own**:
  - Cross-VM / multi-node distributed routing guarantees (future tasks).
  - QUIC, mDNS, NAT traversal, relay/proxying.
  - Kernel crypto/parsers or kernel networking features.
  - A kernel identity system; identity and attestation remain in userland services/libraries.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC MUST link to the task(s) that implement and prove each phase/milestone.
- For this RFC, execution truth is `TASK-0003` (host tests + OS/QEMU markers). OS/QEMU work is gated by `TASK-0010` feasibility.
- See `TASK-0003` section **“Stop conditions (Definition of Done)”** for the canonical proof gates.

## Context

`userspace/dsoftbus` is the distributed fabric (ADR‑0005). `docs/distributed/dsoftbus-lite.md` describes the host-first split:

- host backend can be deterministic without real sockets,
- OS backend is currently stubbed until the OS has a minimal sockets facade.

`TASK-0003` defines the first OS networking milestone as “local only”: discovery + session establishment within a single VM or local subnet. This RFC seeds the contract for that OS transport layer while keeping scope bounded and proofs deterministic.

## Goals

- Define the minimal OS DSoftBus transport contract for:
  - **discovery** (UDP-based),
  - **sessions** (TCP-based),
  - **authentication** (Noise handshake reuse from host backend),
  - **identity checks** (device identity binding).
- Keep transport behavior deterministic and bounded.
- Keep OS/QEMU proof explicitly gated on `TASK-0010` + networking availability.

## Non-Goals

- Cross-VM distributed sessions and discovery across multiple QEMU VMs.
- QUIC, mDNS, or any “internet-grade” networking.
- Kernel participation in crypto, parsing, or DSoftBus framing.

## Constraints / invariants (hard requirements)

- **Tasks are execution truth**: stop conditions + proof commands live in `TASK-0003`.
- **Determinism**:
  - host tests must be deterministic (no flaky sockets),
  - OS backend must use the sockets facade’s polling/timer model (RFC‑0006).
- **No fake success**:
  - no “ok/ready” markers unless the real discovery/session/auth flow happened (per `TASK-0003`).
- **Bounded resources**:
  - discovery packet size bounded,
  - connection/session limits explicit,
  - per-session read/write buffers bounded.
- **Identity binding**:
  - any security-critical identity claims must not be trusted solely from payload bytes; follow RFC‑0005 extension policy principles (bind to authoritative local identity sources; avoid confused deputy patterns).

## Proposed design

### Contract / interface (normative)

This RFC defines DSoftBus transport behavior in terms of the sockets facade (RFC‑0006). It does not define a new kernel ABI.

#### Discovery (UDP) — “local only” milestone

Normative expectations:

- DSoftBus OS transport SHALL:
  - bind a UDP socket on a well-known port (or a configured port for tests),
  - periodically emit an announcement packet on the local subnet,
  - listen for peer announcements and publish them to the discovery layer.

Scope constraints:

- “Local only” includes:
  - loopback (for self-connect tests),
  - local subnet discovery (single broadcast/multicast domain).
- Excludes:
  - cross-VM orchestration harness,
  - multi-hop routing,
  - mDNS/SSDP compatibility.

Packet content (seed-level; must be versioned):

- must include:
  - protocol magic + version,
  - device identifier (stable),
  - session port,
  - Noise static public key (or a stable reference to it),
  - minimal capability flags (optional, bounded).

Normative packet v1 (byte layout):

- Canonical implementation + golden vectors:
  - `userspace/dsoftbus/src/discovery_packet.rs`
  - `userspace/dsoftbus/tests/discovery_packet.rs`
- Versioning:
  - Packets are versioned; v1 is identified by `magic="NXSB"` and `version=1`.
  - Any incompatible change MUST create a new version; v1 decode MUST remain deterministic.
- Bounds (v1):
  - `device_id` is UTF‑8, length \(1..=64\).
  - `services` count \(0..=16\).
  - each service name is UTF‑8, length \(1..=64\).
- Layout (big-endian integers):
  - bytes 0..4: magic `NXSB`
  - byte 4: version `1`
  - byte 5: `device_id_len` (u8)
  - bytes 6..(6+len): `device_id` (UTF‑8)
  - next 2 bytes: `port` (u16, BE)
  - next 32 bytes: `noise_static` (32 bytes)
  - next 1 byte: `service_count` (u8)
  - repeated `service_count` times:
    - 1 byte: `service_len` (u8)
    - `service_len` bytes: `service` (UTF‑8)

#### Sessions (TCP) — “local only” milestone

Normative expectations:

- Transport SHALL establish a TCP connection to a discovered peer (or loopback) and run the DSoftBus handshake over it.
- Transport SHALL expose an authenticated session to the upper layers (session + framed streams).

#### Authentication + identity checks

Normative expectations:

- Authentication SHALL reuse Noise XK handshake logic already present in `userspace/dsoftbus` (host backend).
- Identity checks SHALL:
  - bind the peer session to a stable peer identity (device id / key),
  - reject mismatches deterministically (no “accept then warn”).

Explicit non-goal: kernel crypto/parsers. All handshake and framing stays in userland.

### Phases / milestones (contract-level)

- **Phase 0 (Host-first)**:
  - Deterministic host tests prove handshake + ping/pong and auth-failure **without relying on OS network sockets** (socketless, in-process transport).
  - Proof is via `TASK-0003` host tests.
  - The host multi-node harness (`tests/remote_e2e`) runs over the sockets facade contract (`userspace/nexus-net` with `FakeNet`) and exercises **on-wire discovery (announce v1)** plus TCP sessions.
  - Contract alignment: host tests also cover a DSoftBus transport layered over the sockets facade contract (`userspace/nexus-net` with `FakeNet`).
  - Preferred regression proof remains `just test-e2e` / `just test-host` (CI parity; see `docs/testing/index.md`).
- **Phase 1 (OS/QEMU)**:
  - Implement OS backend over the sockets facade (RFC‑0006) with UDP discovery + TCP sessions.
  - Emit the `TASK-0003` required markers.
  - This phase is gated on `TASK-0010` (safe MMIO → virtio-net → smoltcp → sockets).
- **Phase 2 (Follow-ups)**:
  - discovery hardening, rate limiting, multi-node harness, additional transports.

## Security considerations

- **Threat model**: spoofed discovery announcements, replay, resource exhaustion, confused deputy between identity and transport.
- **Mitigations**:
  - Noise handshake provides authenticated encryption and peer authentication.
  - Identity binding rules must be explicit and deterministic; align with RFC‑0005 extension policy principles (no “trust requester string” equivalents).
  - Bounded resources + backpressure (no unbounded allocations on malformed input).
- **Open risks**:
  - “Local only” discovery is inherently forgeable on a shared subnet; the handshake must be the authorization boundary for sessions.

## Failure model (normative)

- Malformed discovery packets are ignored with deterministic counters/metrics (if present), not crashes.
- Session handshake failures return deterministic errors and do not create “half-open” authenticated state.
- Ping/pong semantics must be bounded and must not busy-loop.

Discovery packet parsing (v1, deterministic):

- Parsing MUST be total (no panics): invalid inputs return deterministic errors.
- Required error classes for v1 (see `userspace/dsoftbus/src/discovery_packet.rs`):
  - `BadMagic`, `UnsupportedVersion`, `Truncated`, `InvalidInput`, `Utf8`.
- OS backend behavior:
  - On malformed packets: ignore (do not crash; may increment a bounded counter).
  - On valid packets: publish peer announcement deterministically.

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

### Proof (OS/QEMU) (gated on TASK-0010 / networking)

Preferred workflow (CI parity; wrapper around `scripts/qemu-test.sh`):

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s just test-os
```

Narrow proof (task harness; equivalent to `just test-os`):

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

## Alternatives considered

- Implement discovery/session in kernel: rejected (TCB growth; violates “userland transport” direction).
- Use QUIC/mDNS: rejected for the local-only milestone (scope + determinism).

## Open questions

- Exact discovery addressing mode (broadcast vs multicast) for the first OS backend, and how to keep it deterministic under QEMU backends.
- How OS identity material (device keys) is provisioned to `dsoftbusd` without expanding kernel TCB.

## Checklist (keep current)

- [x] Scope boundaries are explicit; cross-RFC ownership is linked (RFC‑0006 + ADR‑0005 + RFC‑0005).
- [x] Task(s) exist for each milestone and contain stop conditions + proof (`TASK-0003`; Phase 1 gated on `TASK-0010`).
- [x] Proof is “honest green” (tests/markers), not log-grep optimism (host: `cargo test -p dsoftbus`, preferred: `just test-e2e` / `just test-host`).
- [x] Determinism + bounded resources are specified (local-only milestone; deterministic host transports; bounded frames).
- [x] Security invariants are stated and have at least one regression proof (Noise handshake + identity checks exercised in host tests).
- [x] Discovery announce packet v1 on-wire seed is versioned, bounded, and has golden vectors (host-first; see `userspace/dsoftbus/{src,tests}/discovery_packet.rs`).
- [ ] Other on-wire surfaces (future discovery extensions / additional packet versions) have golden vectors + compat tests. (Not claimed yet.)
- [x] Stubs (if any) are explicitly labeled and non-authoritative (OS backend returns `Unsupported` deterministically; QEMU markers gated).

### Phase 0 implementation checklist (host-first transport proof)

- [x] Socketless deterministic transport tests exist: `userspace/dsoftbus/tests/host_transport.rs` (in-process transport).
- [x] Sockets-facade alignment tests exist: `userspace/dsoftbus/tests/facade_transport.rs` (over `userspace/nexus-net` `FakeNet`).
- [x] Multi-node harness uses facade contract (no flaky loopback sockets): `tests/remote_e2e` over `FakeNet`.
- [x] Host-first UDP discovery over sockets facade is proven deterministically (`userspace/dsoftbus/src/facade_discovery.rs` + `userspace/dsoftbus/tests/facade_discovery.rs`).
  - Includes cache-seeded watch + multi-announce coverage (see `facade_discovery_*` tests).
- [x] Discovery announce packet v1 is versioned + bounded and has golden-vector tests (`userspace/dsoftbus/src/discovery_packet.rs`, `userspace/dsoftbus/tests/discovery_packet.rs`).
- [x] OS backend stubs are “honest”: `userspace/dsoftbus/src/os.rs` returns `Unsupported` / deterministic errors (no `todo!()` panics).
- [x] Preferred CI-parity workflows are documented and green: `just test-e2e` and `just test-host`.

### Phase 1 implementation checklist (OS/QEMU transport)

- [x] OS/QEMU networking prerequisite markers are gated in `scripts/qemu-test.sh` (see Phase 1 prereqs above).
- [ ] `userspace/dsoftbus` compiles for OS (`no_std`/`alloc`) with a real sockets facade backend (no `std::net` / no panics in OS path).
- [x] `dsoftbusd` service is wired into os-lite init and emits:
  - `dsoftbusd: os transport up (udp+tcp)` (bounded announce/recv v1 over netstackd UDP facade; TCP session port)
  - `dsoftbusd: auth ok` (bounded identity gate: client pub + server pub + nonce-derived tag before session)
  - `dsoftbusd: os session ok`
- [ ] OS backend supports (real DSoftBus transport, not just loopback ping/pong):
  - UDP discovery announce + receive (local subnet scope; structured v1 payload bounded)
  - TCP connect + accept (over sockets facade)
  - Noise XK handshake + identity checks (userland) — deferred to `tasks/TASK-0003B-dsoftbus-noise-xk-os.md` (current milestone uses a bounded client+server-pub + nonce-derived tag gate)
- [x] Selftest performs a bounded connect + ping/pong against the OS backend and emits:
  - `SELFTEST: dsoftbus os connect ok`
  - `SELFTEST: dsoftbus ping ok`
