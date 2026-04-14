# Networking Authority: Canonical vs Alternative Paths

**Created**: 2026-01-07  
**Owner**: @runtime  
**Status**: Active

## Purpose

This document clarifies the networking architecture authority to prevent drift between parallel implementations.

Address-profile authority (subnets/IPs/gateways/DNS targets) is tracked in
`docs/architecture/network-address-matrix.md`.

---

## Canonical Networking System (Primary)

**Authority**: `TASK-0003` (Networking step 1: virtio-net + smoltcp + DSoftBus)

**Services**:
- `netstackd` (smoltcp-based, owns virtio-net MMIO, exports IPC sockets facade)
- `dsoftbusd` (DSoftBus distributed networking)

**Use-case**:
- DSoftBus distributed networking (discovery, sessions, streams)
- Primary networking stack for Neuron OS
- Production path

**RFC Contracts**:
- RFC-0006: Userspace Networking v1 (sockets facade)
- RFC-0007: DSoftBus OS Transport v1 (UDP discovery + TCP sessions)
- RFC-0008: DSoftBus Noise XK v1 (handshake + identity binding)

**Feature Gate**: `cfg(feature = "net-canonical")`

### QEMU smoke vs 2-VM harness (determinism)

The **single-VM QEMU smoke** path is a bounded, deterministic-ish wiring proof. To reduce flakiness:

- The smoke harness validates `net: smoltcp iface up ...` as the default “iface configured” proof.
- DHCP and DSoftBus proofs are optional and can be explicitly required via harness flags:
  - `REQUIRE_QEMU_DHCP=1`
  - `REQUIRE_DSOFTBUS=1`
- When DHCP is unavailable in single-VM smoke, `netstackd` may fall back to `10.0.2.15/24`
  (slirp/usernet convention) to keep loopback-based DSoftBus bring-up deterministic.

The **2-VM harness** (`just os2vm`) remains the canonical proof for cross-VM discovery/sessions and
must not depend on slirp/usernet DHCP.

Concrete profile values are maintained in `docs/architecture/network-address-matrix.md`.

---

## Alternative Path: Bring-up Lite (Secondary)

**Authority**: `TASK-0248` / `TASK-0249` (RISC-V Bring-up v1.2)

**Services**:
- `virtionetd-lite` (lightweight virtio-net frontend)
- `netstackd-lite` (DHCP stub + loopback only, no smoltcp)
- `fetchd` (HTTP-like client for smoke tests)
- `echod` (UDP echo server)

**Use-case**:
- Bring-up smoke tests (deterministic, minimal)
- Simple fetch/echo without full DSoftBus stack
- NOT a replacement for canonical networking

**Naming Convention**:
- All services use `-lite` suffix to prevent name collision

**Feature Gate**: `cfg(feature = "net-bringup-lite")`

---

## Authority Rules (Anti-Drift)

### 1. Mutual Exclusion
- Compile-time feature gates ensure only ONE path is active
- `cfg(feature = "net-canonical")` XOR `cfg(feature = "net-bringup-lite")`
- Boot profile (`nexus-init`) explicitly chooses one path

### 2. Default Path
- **Default**: Canonical (`net-canonical`)
- Alternative path is opt-in only

### 3. Name Collision Prevention
- Canonical services: `netstackd`, `dsoftbusd` (no suffix)
- Alternative services: `*-lite` suffix (e.g. `netstackd-lite`, `virtionetd-lite`)

### 4. Cross-References
- All networking tasks MUST explicitly state which path they belong to
- Tasks MUST link to this document for authority clarification

### 5. Address Matrix Authority
- Subnet/IP/DNS/gateway choices are normative only when documented in
  `docs/architecture/network-address-matrix.md`.
- Any profile change requires ADR + matrix + test/harness updates in the same change set.

---

## Task Mapping (Updated 2026-04-14)

| Task | Path | Services | Status |
|------|------|----------|--------|
| TASK-0003 | Canonical | `netstackd`, `dsoftbusd` | ✅ Done |
| TASK-0003B | Canonical | (Noise XK handshake) | ✅ Done (loopback scope) |
| TASK-0003C | Canonical | (UDP discovery) | ✅ Done (loopback scope) |
| TASK-0004 | Canonical | (dual-node + identity binding) | ✅ Done |
| TASK-0005 | Canonical | (cross-VM DSoftBus + remote proxy) | ✅ Done (opt-in 2-VM harness) |
| TASK-0021 | Canonical (host-first) | (QUIC v1 selection + host transport proof, OS fallback markers) | 🟨 In Progress |
| TASK-0023 | Canonical (OS QUIC) | (OS QUIC enablement gate) | Blocked |
| TASK-0024 | Canonical | (UDP-sec OS transport path, non-QUIC) | Draft |
| TASK-0248 | Alternative | `virtionetd-lite`, `netstackd-lite` | Draft |
| TASK-0249 | Alternative | (OS wiring for lite services) | Draft |

**Note**: TASK-0003B/C are "Done" for loopback scope. Discovery-driven TCP connect + identity binding enforcement are completed in TASK-0004; cross-VM proof is completed in TASK-0005 and remains opt-in (2× QEMU).

---

## Decision History

### 2026-01-07: Authority Clarification
- **Problem**: TASK-0003 and TASK-0248/0249 both defined `netstackd` → name collision
- **Decision**: TASK-0003 is canonical; TASK-0248/0249 renamed to `*-lite`
- **Rationale**: TASK-0003 is the primary networking milestone with RFC contracts (RFC-0006, RFC-0007)
- **Impact**: All downstream tasks (0004, 0005, 0024) depend on canonical path

### 2026-03-24: Address Profile Consolidation
- **Problem**: address decisions risked drifting across service/runtime/harness docs.
- **Decision**: keep one normative matrix in `docs/architecture/network-address-matrix.md`.
- **Impact**: networking and distributed docs now reference the same profile table.

### 2026-04-14: QUIC task boundary clarification
- **Problem**: QUIC ownership drift between host-first scaffold and OS enablement follow-on tasks.
- **Decision**: map host-first QUIC to `TASK-0021`; keep OS QUIC enablement explicitly gated in `TASK-0023`.
- **Impact**: authority table now reflects real execution boundaries and avoids accidental scope absorption.

---

## Future Considerations

### When to Use Alternative Path?
- Bring-up environments where full DSoftBus stack is not needed
- Deterministic smoke tests (fetch/echo) without discovery/sessions
- Debugging/profiling minimal networking without distributed fabric

### When to Use Canonical Path?
- Production systems
- Any task requiring DSoftBus (discovery, sessions, streams)
- Any task requiring RFC-0006/0007 contracts

### Migration Path
- If alternative path proves superior: explicitly deprecate canonical and migrate
- If canonical path is complete: alternative path can be removed or kept as opt-in
- Do NOT maintain both paths indefinitely without clear use-case separation
