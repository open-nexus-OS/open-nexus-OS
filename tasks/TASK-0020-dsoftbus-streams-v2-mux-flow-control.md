---
title: TASK-0020 DSoftBus Streams v2: multiplexing + flow control + keepalive (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Unblocks: tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md
  - Unblocks: tasks/TASK-0017-dsoftbus-remote-statefs-rw.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Current DSoftBus framing supports a single reliable stream with a `channel: u32` field, but it does
not provide:

- per-stream flow control,
- robust backpressure,
- priorities/fair scheduling,
- keepalive/heartbeat,
- clean open/accept lifecycle for multiple independent logical streams.

Remote services (remote-fs, samgr/bundlemgr proxies) will benefit from a yamux-like MUX layer.

## Goal

Provide a robust stream-multiplexing layer over the existing authenticated transport so that:

- multiple logical streams can be opened/accepted independently,
- streams have priorities and fair scheduling,
- window-based flow control provides backpressure (no unbounded buffering),
- keepalive detects dead peers deterministically.

## Non-Goals

- Full yamux protocol compatibility.
- QUIC or datagram transport.
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Bounded memory**:
  - cap number of concurrent streams,
  - cap per-stream receive buffer,
  - cap max frame payload size (chunking required).
- **Determinism**:
  - keepalive intervals and timeouts are bounded and deterministic,
  - tests do not rely on “wall clock jitter” for correctness.
- **No fake success**: markers only emitted after real multiplexed data transfer.
- **No async runtime requirement**: prefer a pump/poll API that works in host tests and OS bring-up
  without pulling in an async executor.

## Red flags / decision points

- **RED (blocking)**:
  - `userspace/dsoftbus` OS backend is currently a placeholder (`userspace/dsoftbus/src/os.rs`).
    Note: OS bring-up streams exist via os-lite services (`netstackd` + `dsoftbusd`) as of TASK-0005,
    but the reusable library backend remains stubbed until TASK-0022 refactors DSoftBus core for OS/no_std.
    This task must therefore be **host-first** with OS-gated follow-up steps.
- **YELLOW (risky)**:
  - Scheduling fairness can become flaky if expressed in terms of “milliseconds”. Prefer deterministic
    byte/credit accounting and ordering constraints.
  - Priority mapping must be conservative: “high priority” must not starve lower priority streams
    indefinitely (bounded starvation).

## Contract sources (single source of truth)

- DSoftBus high-level traits: `userspace/dsoftbus` (`Stream::send/recv`, channel+bytes model)
- QEMU marker contract: `scripts/qemu-test.sh` (only once OS backend exists)

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (new crate `tests/dsoftbus_mux_host` or under `userspace/dsoftbus` tests):

- interleaving with priorities (high-pri favored, but low-pri progresses),
- slow consumer enforces backpressure (writer blocks/returns WouldBlock; no OOM),
- keepalive timeout (missing PONG tears down),
- RST propagation,
- fuzz-ish deterministic state-machine test (seeded) for frame ordering/credit invariants.

### Proof (OS / QEMU) — gated on OS backend

Once OS DSoftBus streams exist:

- extend `scripts/qemu-test.sh` with:
  - `dsoftbus:mux session up`
  - `dsoftbus:mux data ok`
  - `SELFTEST: mux pri control ok`
  - `SELFTEST: mux bulk ok`
  - `SELFTEST: mux backpressure ok`

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (mux module + integration points)
- `source/services/dsoftbusd/` (use mux after handshake; OS-gated)
- `source/apps/selftest-client/` (OS-gated mux markers)
- `tests/` (host mux tests)
- `docs/distributed/`
- `scripts/qemu-test.sh` (only when OS backend is ready)

## Plan (small PRs)

1. **Mux protocol + engine (host-first)**
   - Implement a versioned frame format with:
     - OPEN / OPEN_ACK
     - DATA
     - WINDOW_UPDATE
     - RST
     - PING / PONG
     - CLOSE
   - Flow control: per-stream credit (default 64KiB), sender decrements on DATA, receiver issues WINDOW_UPDATE on consume.
   - Priorities: 0..7 with a simple scheduler (strict priority + round-robin within each level), plus a starvation bound.

2. **Integration into host backend**
   - After Noise handshake, wrap the transport in a `MuxSession`.
   - Provide `open_stream(name, pri)` and `accept_stream()` registry on top of mux streams.

3. **Migrate client/server protocols**
   - Move remote-fs and proxy traffic onto named mux streams with chunking (<= 32KiB per DATA).
   - **RPC Format Note**: TASK-0005/0016/0017 use OS-lite byte frames as bring-up shortcuts. This task is a good migration point to introduce schema-based RPC (Cap'n Proto) on top of mux streams. See TASK-0005 "Technical Debt" section.

4. **OS integration (gated)**
   - Once OS DSoftBus is real, adopt the same mux session in OS build and add QEMU markers/selftest.

## Docs

- Add `docs/distributed/dsoftbus-mux.md` describing:
  - frame format
  - flow control rules
  - priority policy and starvation bound
  - keepalive behavior
  - limits (max streams, max frame size)

## Alignment note (2026-02, low-drift)

- Current OS-lite cross-VM path stabilizes connection lifecycle with an explicit session FSM + epoch ownership
  in `dsoftbusd` (no kernel/protocol contract change).
- Session setup now uses bounded, transport-level readiness checks before stream writes (`WouldBlock` remains the
  backpressure signal).
- Discovery receive polling in setup is rate-limited and non-fatal once peer mapping is known, so session progress
  is not starved by discovery RPC timing.
- Mux v2 should treat these as transport invariants and keep stream-level scheduling/credits orthogonal (no
  incompatible buffering semantics).
