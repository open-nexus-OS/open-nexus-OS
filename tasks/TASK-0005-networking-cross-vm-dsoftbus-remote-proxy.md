---
title: TASK-0005 Networking step 3 (OS): cross-VM DSoftBus sessions + remote proxy (samgr/bundlemgr) + 2-VM harness (opt-in)
status: Done
owner: @runtime
created: 2025-12-22
updated: 2026-01-13
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on: tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md
  - Follow-on: tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Follow-on: tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md
  - Follow-on: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Follow-on: tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
  - RFC: docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
  - RFC: docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md
---

## Context

Step 1/2 get OS networking and DSoftBus working locally. Step 3 makes it real:

- cross-VM discovery and sessions (OS↔OS) using subnet multicast/broadcast discovery,
- and a minimal **remote proxy** so a node can call *remote* `samgrd` (resolve) and *remote* `bundlemgrd` (query/list),
  using an authenticated DSoftBus stream as the transport.

This task is opt-in for CI because it spins up **two QEMU instances**.

## Goal

With **kernel unchanged**, prove in an opt-in 2-VM run:

- Node A discovers Node B and establishes a Noise-authenticated DSoftBus session cross-VM.
- Node A can remotely resolve a service via Node B’s `samgrd` and query bundles via Node B’s `bundlemgrd`.

## Non-Goals

- Kernel networking changes.
- Full distributed service graph (global registry, remote capabilities, remote VMO, etc.).
- mDNS compliance (we only need “mDNS-style” multicast/bcast discovery semantics).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **No fake success**: cross-VM markers only after real discovery/session + remote call roundtrips.
- **Determinism**:
  - marker strings stable;
  - discovery announce schedule must be deterministic (no RNG jitter); timeouts bounded.
- **Protocol drift avoidance**:
  - Remote proxy must reuse the **existing OS-lite on-wire frames** for `samgrd` and `bundlemgrd`
    (do not invent a parallel Cap’n Proto protocol unless we explicitly choose to migrate).
- **Proxy authority is explicit and narrow**:
  - The “remote gateway” is effectively a privileged proxy surface. It must be deny-by-default and
    only forward the minimal samgr/bundlemgr operations required by this task.
  - No capability transfer across the DSoftBus boundary in this step (no “remote caps”); only
    request/response bytes.

## Important feasibility note (2-VM network backend)

Two separate QEMU instances do not automatically share "usernet" (slirp). For a deterministic, rootless
2-VM harness we should prefer a QEMU backend that actually links the NICs, e.g.:

- `-netdev socket,mcast=239.42.0.1:37020` (both VMs join the same L2 multicast hub), or
- `-netdev socket,listen=...` / `connect=...` (pair link).

If we rely on slirp/usernet, cross-VM multicast/broadcast will likely not work and the task will be flaky.

## Prerequisites / blocking conditions (make “real subnet” explicit)

TASK-0004 proves discovery/session correctness in **single-VM bring-up** using deterministic **UDP loopback**.
This task (step 3) explicitly requires **real UDP datagram behavior across two VMs**:

- **QEMU networking backend MUST actually connect the two NICs at L2/L3**
  - Preferred: `-netdev socket,mcast=239.42.0.1:37020` (both VMs on same multicast hub), or
  - Alternative: `-netdev socket,listen=...` / `connect=...` (pair link).
  - **Do NOT** assume slirp/usernet provides cross-VM broadcast/multicast.

- **Guest networking MUST provide real UDP socket semantics for discovery**
  - Discovery must run over a real UDP socket backend (datagram-framed), not the netstackd UDP loopback byte-ring.
  - If multicast group join is not supported in the current smoltcp/netstack layer, the task MUST document and use
    a deterministic fallback (e.g. broadcast on the shared L2, or controlled unicast after first contact) — but still
    cross-VM, not loopback.

- **Identity binding is mandatory before remote proxy**
  - Step 3 MUST NOT forward any remote proxy request until the session is Noise-authenticated **and identity bound**
    (device_id ↔ noise_static_pub mapping enforced).

If any of the above cannot be satisfied without kernel changes, this task is blocked and MUST spin out a new task/RFC
seed for the missing netstack/multicast/datagram capability, instead of “papering over” with fake-green markers.

## Security considerations

### Threat model

- **Cross-VM session hijacking**: Attacker on network intercepts or injects DSoftBus traffic
- **Unauthorized remote access**: Malicious node attempts to access services without proper authentication
- **Privilege escalation via remote proxy**: Attacker uses remote gateway to access local services
- **Data exfiltration**: Sensitive data from `samgrd`/`bundlemgrd` exposed to unauthorized remote nodes
- **Replay attacks on remote calls**: Old request/response pairs replayed
- **Man-in-the-middle on 2-VM link**: Attacker on shared L2 segment intercepts traffic

### Security invariants (MUST hold)

- All cross-VM communication MUST be over Noise XK authenticated streams (no plaintext)
- Remote proxy MUST verify session authentication before forwarding any request
- Remote proxy MUST be deny-by-default: only explicitly allowed services (`samgrd`, `bundlemgrd`) are proxied
- No capability transfer across DSoftBus boundary (request/response bytes only)
- Remote requests MUST be bounded in size and rate-limited
- Session keys MUST NOT be logged or exposed in error messages

### DON'T DO

- DON'T forward requests to services not explicitly allowlisted in remote proxy
- DON'T transfer kernel capabilities across the network boundary
- DON'T accept unbounded request sizes from remote peers
- DON'T log request/response payloads containing potentially sensitive data
- DON'T skip authentication for "trusted" network segments
- DON'T allow remote proxy to escalate privileges beyond the authenticated peer's identity

### Attack surface impact

- **Significant**: Remote gateway is a privileged proxy surface (highest risk in this task)
- **Significant**: 2-VM harness exposes L2 multicast traffic to potential interception
- **New attack vector**: Remote `samgrd`/`bundlemgrd` access from external nodes

### Mitigations

- Noise XK provides authenticated encryption for all cross-VM traffic
- Remote gateway is deny-by-default with explicit allowlist (`samgrd`, `bundlemgrd` only)
- Request size bounded; oversized requests rejected
- All proxied requests logged for audit (service, peer ID, operation)
- No capability transfer: only request/response bytes cross the boundary
- Identity binding verified before any remote call is processed

## Security proof

### Audit tests (negative cases)

- Command(s):
  - `cargo test -p dsoftbus -- reject_remote --nocapture`
- Required tests:
  - `test_reject_remote_unauthenticated_remote_call` — no session → rejected
  - `test_reject_remote_disallowed_service_proxy` — service not in allowlist → rejected
  - `test_reject_remote_oversized_remote_request` — bounded input enforced
  - `test_reject_remote_audit_remote_call_logged` — all remote calls produce audit record

### Hardening markers (QEMU)

- `dsoftbusd: remote proxy denied (service=<svc>)` — deny-by-default works
- `dsoftbusd: remote proxy denied (unauthenticated)` — auth required
- `dsoftbusd: remote proxy ok (peer=<id> service=<svc>)` — audit trail

## Contract sources (single source of truth)

- **Single-VM marker contract**: `scripts/qemu-test.sh` must remain green by default (no regressions).
- **OS-lite service protocols (authoritative for this step)**:
  - `samgrd` v1 frames: `source/services/samgrd/src/os_lite.rs` (`SM` magic)
  - `bundlemgrd` v1 frames: `source/services/bundlemgrd/src/os_lite.rs` (`BN` magic)

## Stop conditions (Definition of Done)

### Proof (default / single VM)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` remains green (step 1/2 markers intact).

### Proof (opt-in / two VMs)

Provide a new **canonical harness** (not a “postflight”):

- `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`

The harness must fail unless both VMs produce:

- `dsoftbusd: discovery cross-vm up`
- `dsoftbusd: cross-vm session ok <peer>`
- `SELFTEST: remote resolve ok`
- `SELFTEST: remote query ok`

Marker ownership rules (to keep the harness deterministic):

- `SELFTEST: remote resolve ok` and `SELFTEST: remote query ok` must be emitted by **Node A only**
  (the “client” VM).
- `dsoftbusd: cross-vm session ok <peer>` is expected on **both** nodes once the session is up.

## Implementation status (2026-01-13)

- **Completion**: 100%
- **Verified**:
  - `just dep-gate && just diag-os && just diag-host && cargo test --workspace`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` (opt-in 2× QEMU proof)

## Touched paths (allowlist)

- `userspace/dsoftbus/` (OS backend: cross-VM discovery/session hardening)
- `source/services/dsoftbusd/` (OS daemon wiring + markers + remote-gateway handler)
- `source/services/netstackd/` (2-VM harness: deterministic non-DHCP networking fallback)
- `userspace/nexus-net-os/` (static IPv4 fallback helper + MAC-derived determinism)
- `source/services/samgrd/` (add cap-free resolve-status op for remote proxy)
- `source/apps/selftest-client/` (cross-VM mode markers)
- `tools/os2vm.sh` (2-VM harness; canonical for opt-in proof)
- `scripts/run-qemu-rv64.sh` (if needed to parameterize per-instance UART logs / netdev args)
- `docs/` (dsoftbus-os + testing)

## Plan (small PRs)

1. **Cross-VM discovery payload + aging**
   - Discovery message includes:
     - device id
     - TCP port
     - protocol version
     - services list (e.g. `["samgrd","bundlemgrd"]`)
   - Peer IP SHOULD be taken from the UDP `recv_from` source address where possible (avoid adding new on-wire fields).
     If we truly need an explicit “advertised IP” field (NAT/backends), that MUST be introduced as a versioned packet
     update with its own contract seed (do not silently extend the v1 packet).
   - Maintain bounded LRU with TTL aging; refresh on new announce.
   - Marker: `dsoftbusd: discovery cross-vm up` once sockets are bound and RX loop is active.

2. **Cross-VM session establishment**
   - Prefer TCP to the peer’s advertised IP:port when peer != localhost.
   - Reuse Noise XK handshake + identity checks from the host backend logic.
   - Marker: `dsoftbusd: cross-vm session ok <peer>` only after the authenticated stream is established.

3. **Remote proxy (samgr/bundlemgr)**
   - Implement a small “remote gateway” over DSoftBus streams:
     - Receives framed requests on an authenticated channel.
     - Forwards them to local services using existing kernel IPC routing (samgrd/bundlemgrd OS-lite endpoints).
     - Returns the raw response bytes to the remote peer.
   - The forwarded request/response bytes are the **existing OS-lite frames**:
     - `samgrd` v1 (`SM`): resolve/lookup
     - `bundlemgrd` v1 (`BN`): list/query/image fetch (as supported)
   - Where to place this:
     - Preferred: inside `dsoftbusd` as a dedicated channel handler (“remote-gateway”), to avoid modifying
       samgrd/bundlemgrd in this step.

4. **Selftest: cross-VM mode**
   - Default mode (single VM) remains unchanged.
   - Cross-VM mode is enabled via env (e.g. `OS2VM=1`) set by the 2-VM harness.
   - Selftest waits for a non-self peer, then:
     - remote resolve via `samgrd` (`SELFTEST: remote resolve ok`)
     - remote bundle query via `bundlemgrd` (`SELFTEST: remote query ok`)

5. **2-VM harness (opt-in, canonical)**
   - Add `tools/os2vm.sh` that:
     - boots **two** QEMU instances with distinct UART logs (e.g. `uart-A.log`, `uart-B.log`)
     - configures networking via a deterministic, rootless backend (`-netdev socket,mcast=...` preferred)
     - sets distinct device ids/ports via env/args
     - waits for the required markers (bounded time), trims logs, exits non-zero on failure
   - This harness is the single source of truth for the 2-VM proof (no separate “postflight” script).

6. **Docs**
   - Extend `docs/distributed/dsoftbus-os.md` with a cross-VM section:
     - discovery payload fields, TTL/aging, session policy
     - remote gateway semantics and limitations
   - Extend `docs/testing/index.md`:
     - how to run `tools/os2vm.sh`, expected logs, troubleshooting

## Acceptance criteria (behavioral)

- Default single-VM run remains green (`scripts/qemu-test.sh` unchanged except for any intentional new markers in step 3 being opt-in only).
- Opt-in 2-VM run (`RUN_OS2VM=1 tools/os2vm.sh`) proves:
  - cross-VM discovery + Noise-authenticated session
  - remote resolve + remote query markers
- No kernel changes.

## Evidence (to paste into PR)

- Attach `uart-A.log` and `uart-B.log` tails showing:
  - `dsoftbusd: discovery cross-vm up`
  - `dsoftbusd: cross-vm session ok ...`
  - `SELFTEST: remote resolve ok`
  - `SELFTEST: remote query ok`

## RFC seeds (for later, once green)

- Decisions made:
  - Cross-VM discovery payload format + TTL semantics.
  - Rootless 2-VM QEMU networking backend choice and limitations.
  - Remote-gateway placement (dsoftbusd vs services) and on-wire framing choice.
- Open questions:
  - When/if to migrate remote service calls from OS-lite frames to a stable Cap'n Proto IDL.
  - How to extend beyond samgr/bundlemgr (policy, identity, packagefs/vfs).

---

## ⚠️ Technical Debt: RPC Format Migration Path

**Current state (bring-up shortcut):**

- Remote proxy uses **OS-lite byte frames** (`SM`, `BN` magic) for samgrd/bundlemgrd
- This reuses existing kernel IPC routing without changes

**Target state (production):**

- Migrate to **Cap'n Proto IDL** or equivalent stable schema
- Integrate with QUIC transport (RFC-0007 Phase 3)

**Why this matters:**

- OS-lite frames are undocumented byte formats
- Cap'n Proto provides schema evolution, versioning, and tooling
- OpenHarmony and Fuchsia both use stable IDL (DSoftBus IDL / FIDL)

**Migration trigger:**

- When TASK-0020 (Streams v2 Mux) or TASK-0021 (QUIC) lands
- Remote proxy should be refactored to use schema-based RPC

**Downstream impact:**

- TASK-0016 (Remote PackageFS) — uses same pattern
- TASK-0017 (Remote StateFS) — uses same pattern
- Any new remote service must follow the same migration

**Tracking:** Add to RFC-0007 Phase 2 or create dedicated RFC for "DSoftBus RPC Schema v1"
