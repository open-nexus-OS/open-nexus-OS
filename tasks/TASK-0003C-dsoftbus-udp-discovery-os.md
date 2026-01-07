---
title: TASK-0003C DSoftBus OS UDP Discovery (loopback announce + receive)
status: Done ✅ (loopback scope complete; discovery-driven connect + identity binding → TASK-0004)
owner: @runtime
created: 2026-01-07
updated: 2026-01-07
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
  - Parent task: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on: tasks/TASK-0003B-dsoftbus-noise-xk-os.md
  - Blocks: tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md
---

## Context

TASK-0003 Track B achieved the "loopback milestone" (TCP session + Noise XK handshake working), but the current OS proof only shows **hardcoded `127.0.0.1` connect**, not a "real DSoftBus transport" with UDP discovery and discovery-driven sessions.

## Current Status (2026-01-07)

**RFC-0009 Implementation**: ✅ **Complete** (no_std hygiene fixed)
- Makefile fixed: OS services built with `--no-default-features --features os-lite`
- `parking_lot` / `getrandom` excluded from OS graph
- `just diag-os` ✅, `just diag-host` ✅, `just dep-gate` ✅

**Phase 0 (Build Fix)**: ✅ **Complete**
- `dsoftbusd: ready` ✅
- `dsoftbusd: auth ok` ✅  
- `SELFTEST: dsoftbus ping ok` ✅

**Phase 1 (Discovery Implementation)**: ✅ **COMPLETE (loopback)**

| Stop Condition Marker | Status |
|-----------------------|--------|
| `dsoftbusd: discovery up (udp)` | ✅ |
| `dsoftbusd: discovery announce sent` | ✅ |
| `dsoftbusd: discovery peer found device=local` | ✅ |
| `dsoftbusd: session connect peer=<id>` | ⬜ Requires dual-node (TASK-0004) |
| `dsoftbusd: auth ok` | ✅ |
| `SELFTEST: dsoftbus ping ok` | ✅ |

**Proof Gate (2026-01-07)**:
```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s ./scripts/qemu-test.sh
# Exit code: 0
# "QEMU selftest completed (markers verified)."
```

**Libraries created**:
- `source/libs/nexus-discovery-packet` ✅
- `source/libs/nexus-peer-lru` ✅

**Identity Binding Enforcement**: ⬜ **Deferred** (requires dual-node scenario, TASK-0004)

**What TASK-0003 achieved**:
- ✅ Networking: virtio-net + smoltcp + IPC facade
- ✅ Noise XK: Handshake works (TASK-0003B)
- ✅ TCP session: Loopback connect + ping/pong

**What this task adds** (partial RFC-0007 Phase 1b):
- ✅ UDP discovery announce/receive (OS-side) — loopback scope
- ⬜ Discovery-driven session establishment → **TASK-0004** (requires dual-node)
- ⬜ Identity binding enforcement → **TASK-0004** (requires dual-node for meaningful validation)

This unblocks TASK-0004 (dual-node) and TASK-0005 (cross-VM).

**RFC Alignment**:
- RFC-0007 Phase 1b: UDP Discovery + Discovery-driven Sessions
- RFC-0008 Phase 1b: Identity Binding Enforcement

## Goal (Loopback Scope)

- `dsoftbusd` (OS) binds UDP socket, periodically sends discovery announce v1, receives announcements from peers ✅
- Discovery announcements are parsed, validated, and stored in a bounded peer LRU ✅
- Marker: `dsoftbusd: discovery up (udp)` emitted after UDP socket bind + RX loop active ✅
- Marker: `dsoftbusd: discovery announce sent` emitted after announcement sent ✅
- Marker: `dsoftbusd: discovery peer found device=<id>` emitted when peer discovered ✅

**Note**: Session establishment flow (UDP discovery → peer found → TCP connect to `peer.addr:peer.port` → identity verification) requires **dual-node mode** to be meaningfully tested and is therefore in **TASK-0004**.

## Non-Goals

- Multicast/broadcast subnet-wide discovery (that's TASK-0004; this task can use loopback UDP first)
- DHCP/ICMP (that's TASK-0004)
- Cross-VM proof (that's TASK-0005)

## Constraints / invariants

- no_std + alloc (OS constraints)
- Bounded resources: peer LRU max size, announce rate limit
- Deterministic behavior: no random jitter (use deterministic schedule based on device_id)
- No fake success: `discovery up` only after UDP socket bound and RX loop active

## Red flags / decision points

- **GAP 4 dependency**: This task assumes `userspace/dsoftbus` can compile for OS. If not resolved yet, this task must either:
  - Wait for Gap 4 resolution (refactor `userspace/dsoftbus` for no_std), OR
  - Implement discovery logic directly in `source/services/dsoftbusd` (increases drift risk)

## Contract sources

- QEMU marker contract: `scripts/qemu-test.sh`
- Discovery packet v1: `userspace/dsoftbus/src/discovery_packet.rs` (host-side reference)
- Transport contract: RFC-0007

## Stop conditions (Definition of Done) — LOOPBACK SCOPE

- Proof (QEMU):
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required markers (this task):
    - `dsoftbusd: discovery up (udp)` ✅ (UDP socket bound, RX loop active)
    - `dsoftbusd: discovery announce sent` ✅ (at least one announce sent)
    - `dsoftbusd: discovery peer found device=<id>` ✅ (peer announcement received and parsed)
    - `dsoftbusd: auth ok` ✅ (Noise handshake success — loopback)
    - `SELFTEST: dsoftbus ping ok` ✅ (ping/pong works — loopback)

- **Deferred to TASK-0004** (requires dual-node for meaningful validation):
    - `dsoftbusd: session connect peer=<id>` (discovery-driven TCP connect, not hardcoded loopback)
    - Identity binding enforcement (`device_id <-> noise_static_pub` verification)
    - Session rejection on identity mismatch

- Proof (tests):
  - Host tests for discovery packet parsing/validation ✅ (already exist in `userspace/dsoftbus/tests/discovery_packet.rs`)
  - OS-side libraries created: `nexus-discovery-packet`, `nexus-peer-lru` ✅

## Touched paths (allowlist)

- `source/services/dsoftbusd/**` (UDP discovery logic)
- `userspace/dsoftbus/**` (if refactoring for no_std; see Gap 4)
- `scripts/qemu-test.sh` (marker contract update)
- `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md` (checklist update when done)

## Plan (small PRs)

1. **UDP socket bind + announce send**
   - Bind UDP socket on well-known port (e.g. 37020)
   - Implement periodic announce send (deterministic schedule, no random jitter)
   - Emit marker: `dsoftbusd: discovery up (udp)`
   - Emit marker: `dsoftbusd: discovery announce sent`

2. **UDP announce receive + peer LRU**
   - Implement RX loop for UDP announcements
   - Parse discovery packet v1 (reuse or port host-side parsing logic)
   - Store peers in bounded LRU (max 16 peers)
   - Emit marker: `dsoftbusd: discovery peer found device=<id>`

3. **Discovery-driven session establishment**
   - Replace hardcoded loopback connect with peer selection from LRU
   - Flow: peer found → TCP connect to `peer.addr:peer.port` → Noise handshake
   - Emit marker: `dsoftbusd: session connect peer=<id>`

4. **Identity binding enforcement (Gap 3)**
   - Parse `device_id` + `noise_static_pub` from discovery announcement
   - During Noise handshake: verify peer's authenticated static key matches expected key for `device_id`
   - Create deterministic mapping table (initially: trust-on-first-use for bring-up)
   - Reject session if mismatch (no `auth ok` marker)

5. **Docs + cleanup**
   - Update RFC-0007 checklist (Gap 1, Gap 2, Gap 3 resolved)
   - Update TASK-0003 status to "Done (Track A/B)"

## Acceptance criteria (behavioral)

- UDP discovery proof in QEMU: announcements sent/received, peer found via UDP (not hardcoded)
- Session establishment is discovery-driven (peer selection from LRU)
- Identity binding enforced (session rejected if `device_id ↔ noise_static_pub` mismatch)
- No regressions to existing markers

## Evidence (to paste into PR)

- QEMU (canonical):
  - Command: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `uart.log` tail showing:
    - `dsoftbusd: discovery up (udp)`
    - `dsoftbusd: discovery announce sent`
    - `dsoftbusd: discovery peer found device=<id>`
    - `dsoftbusd: session connect peer=<id>`
    - `dsoftbusd: auth ok`
    - `SELFTEST: dsoftbus ping ok`

## TODO: Dependency on Gap 4 Resolution (userspace/dsoftbus OS compilation)

**Current status (2026-01-07)**: OS-side `dsoftbusd` does NOT use `userspace/dsoftbus` library (see RFC-0007 Gap 4).

**Code Evidence**: `source/services/dsoftbusd/Cargo.toml` has `dsoftbus` dependency only for `not(target_os = "none")`.

**Impact on this task**:
- Discovery logic exists in `userspace/dsoftbus` (host-side)
- But OS-side cannot reuse it (separate implementation required)

**Resolution path**:

### TODO: Gap 4 Resolution (tracked in RFC-0007, TASK-0003)

**Long-term (Option A - recommended for maintainability)**:
- [ ] Audit `userspace/dsoftbus` dependencies (identify `std` blockers)
- [ ] Refactor `userspace/dsoftbus` for `no_std`/`alloc` support
  - Replace `std::net` with trait-based facade (`nexus-net`)
  - Replace `std::sync` with `spin` or `alloc`-based alternatives
  - Feature gates: `std` (default) vs `no_std` (OS targets)
- [ ] Update `dsoftbusd/Cargo.toml` to use library on OS targets
- [ ] Remove duplicate logic from `source/services/dsoftbusd`
- [ ] Update RFC-0007 Gap 4 checklist to `[x]`

**Short-term (Option B - unblock this task immediately)**:
- [x] Port discovery logic from `userspace/dsoftbus` into `dsoftbusd` (accepted as technical debt)
- [x] Create `source/libs/nexus-discovery-packet` (no_std discovery packet codec)
- [x] Create `source/libs/nexus-peer-lru` (no_std bounded peer cache)
- [ ] Create tracking issue: "Reduce host/OS drift by refactoring userspace/dsoftbus for no_std"

**This task proceeds with Option B** to unblock TASK-0004/0005, but Gap 4 resolution remains a priority for long-term maintainability.

## RFC-0009 Implementation Log (2026-01-07)

**Root cause**: Makefile built OS services without `--no-default-features --features os-lite`, causing `parking_lot`/`getrandom` to leak into the bare-metal graph.

**Fix applied**:
1. `Makefile` updated: Line 39 and 53 now use `--no-default-features --features os-lite`
2. Services without `os-lite` feature excluded from OS build: `identityd`, `dist-data`, `clipboardd`, `notifd`, `resmgrd`, `searchd`, `settingsd`, `time-syncd`
3. RFC-0009 status updated to Phase 1 Complete

**Verification**:
- `just diag-os`: ✅
- `just diag-host`: ✅
- QEMU (`RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`):
  - `dsoftbusd: ready` ✅
  - `dsoftbusd: auth ok` ✅
  - `SELFTEST: dsoftbus ping ok` ✅
