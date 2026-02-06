# RFC-0019: IPC Request/Reply Correlation v1 (Nonces + Shared Reply Inbox)

- Status: Complete (v1 contract implemented; proofs green)
- Owners: @runtime
- Created: 2026-02-05
- Last Updated: 2026-02-06
- Links:
  - Tasks: `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` (needs request/reply determinism for QEMU proof)
  - Tasks: `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (logd/crash-report topology baseline)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (CAP_MOVE + endpoint lifecycle)
  - Related RFCs: `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (deterministic markers/phases)
  - Related RFCs: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (crash-report semantics)
  - Engineering: `docs/dev/platform/qemu-virtio-mmio-modern.md` (modern virtio-mmio rationale + patch option)
  - Harness: `scripts/run-qemu-rv64.sh` (canonical QEMU invocation + virtio-mmio policy)

## Status at a Glance

- **Phase 0 (Contract + host runtime + tests)**: ✅
- **Phase 1 (logd proof-path determinism)**: ✅ (bounded ACK consumption + LO v2 nonce frames for multiplexed logd RPCs)
- **Phase 2 (Adopt in statefsd/policyd/execd core flows)**: ✅

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - A versioned, deterministic **request/reply correlation** contract for userspace IPC
  - A minimal **nonce** contract and where it must appear in on-wire frames
  - A bounded **reply dispatcher** contract (shared inbox → matching reply by nonce)
  - Failure/DoS behavior for unmatched replies (bounded buffering, explicit drops)
  - The **QEMU harness policy** required to keep IPC proofs deterministic (modern virtio-mmio default)
- **This RFC does NOT own**:
  - Kernel IPC ABI changes (no new syscalls required)
  - Service-specific semantics (e.g. what logd query means, what statefs put means)
  - Bulk out-of-band transports (VMO/stream subscriptions) beyond this nonce correlation layer

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC defines the correlation contract; tasks adopt it in service protocols and prove it.

## Context

We currently use kernel IPC v1 endpoints + optional CAP_MOVE reply caps. This works for simple,
strictly synchronous call patterns, but becomes flaky under:

- **High fan-in services** (e.g. `logd`) receiving from many senders
- **Shared reply inboxes** (one endpoint receiving replies for multiple concurrent conversations)
- **Cooperative scheduling** (ordering jitter) and bounded queues

Symptoms:

- Send/recv “desynchronizes” (a reply intended for request A is consumed by request B).
- Tests need ad-hoc drains/yields/timeouts that mask structural issues and are not future-proof.

Root cause:

- Missing or inconsistent **conversation IDs** (nonces) at the wire level, and lack of a small,
  bounded **dispatcher** that can route replies to the correct waiter.

This RFC makes request/reply correlation explicit and deterministic so OS/QEMU smoke proofs (and
future OS features) are stable without fragile timing workarounds.

## Goals

- Provide deterministic request/reply correlation for userspace IPC without kernel changes.
- Enable safe multiplexing over a shared reply inbox with bounded memory/CPU.
- Standardize “nonce echo” across services, so generic runtime helpers can be reused.
- Ensure security invariants (identity binding, no spoofing via payload) remain intact.

## Non-Goals

- “At most once” semantics across crashes (handled by per-service logic).
- A general pub/sub system (future RFC).
- Replacing capability-based access control with payload auth (we keep capability/policy model).

## Constraints / invariants (hard requirements)

- **Determinism**: request/reply matching must not depend on timing or queue ordering.
- **Bounded resources**:
  - reply dispatcher uses fixed/bounded storage (ring buffer or bounded Vec)
  - loops have explicit iteration caps and/or deadlines
- **Security floor**:
  - requester identity is derived from kernel metadata (`sender_service_id`) where available
  - nonce is for correlation, not authentication
- **No fake success**: “ok” markers only after the matched reply is actually received and validated.
- **Compatibility**: v1 service protocols may remain supported during bring-up; new correlation must be versioned.
- **QEMU device determinism**: QEMU tests MUST run with modern virtio-mmio by default (legacy opt-in only),
  because legacy virtio-mmio has known nondeterministic/incorrect behavior for virtio rings (notably virtio-blk `used.idx`).

## Proposed design

### Contract / interface (normative)

#### Nonce definition

- **Nonce type**:
  - **Preferred**: `u64` little-endian.
  - **Allowed (compat)**: `u32` little-endian for v1/v2 protocols that already ship with `u32` nonces (e.g. policyd v2).
  - **Rule**: the nonce width is part of the **protocol version contract** and MUST be unambiguous for the decoder.
- **Generation**:
  - MUST be monotonic per client instance (`nonce = nonce + 1`), starting from a stable value.
  - MUST NOT require randomness (`getrandom`) in OS builds.
- **Uniqueness**:
  - Uniqueness is required only within the client’s “in-flight” window.
  - Callers MUST NOT reuse a nonce while a request is in-flight.

#### On-wire correlation rule (“nonce echo”)

For any **request** that expects a **reply** on a shared inbox, the protocol MUST include a nonce and
the server MUST echo it back unchanged.

Normative rule:

- If a request contains a `nonce`, the reply MUST contain the same `nonce`.
- If a server cannot process the request, it still MUST reply with the same `nonce` and an explicit error status.

Versioning rule:

- Adding nonce to an existing protocol MUST be done by **bumping protocol version** (e.g. `LO v2`),
  or by introducing a new op variant (e.g. `OP_QUERY_V2`) with an explicit nonce field.

#### Control-plane determinism (routing + init control messages) (normative)

Some early-boot control channels are **shared** and historically lacked correlation, leading to fragile “drain stale”
workarounds. For v1 bring-up determinism we standardize a minimal, backwards-compatible extension:

- **Routing v1+nonce extension**:
  - Request: `[R,T,1,OP_ROUTE_GET, name_len:u8, name..., nonce:u32le]`
  - Response: `[R,T,1,OP_ROUTE_RSP, status:u8, send_slot:u32le, recv_slot:u32le, nonce:u32le]`
  - Legacy v1 (nonce-less) frames MAY still be accepted during bring-up, but deterministic proofs MUST use the nonce form.
- **Init health control (v1+nonce)**:
  - Request: `[I,H,1,OP_OK, nonce:u32le]`
  - Response: `[I,H,1,OP_OK|0x80, status:u8, nonce:u32le]`

Rationale:

- These control-plane messages are multiplexed on a shared endpoint; correlation is required to avoid consuming the wrong
  response under cooperative scheduling.

#### Reply dispatcher contract (shared inbox)

We standardize a small userspace component (library-level) that:

- receives replies from a shared inbox endpoint,
- validates that they are well-formed for the target protocol,
- extracts the `nonce`,
- returns the reply to the matching waiter (by nonce).

Required behavior:

- **Filtering**: if a reply does not match the awaited nonce, it MUST NOT be dropped silently;
  it MUST be retained in bounded storage for later matching, unless storage is full.
- **Bounding**:
  - storage MUST have a fixed cap (`MAX_PENDING_REPLIES`)
  - if cap is exceeded, the dispatcher MUST drop the oldest entry (or reject new entries) deterministically,
    and increment an explicit drop counter (exposed to tests/logs).
- **Timeout**:
  - awaiting a nonce MUST use explicit budget/deadline loops (see `userspace/nexus-ipc::budget`).

#### Modern virtio-mmio policy (QEMU harness) (normative)

This section defines the **required** harness policy for deterministic IPC/device proofs under QEMU.
It is owned here to prevent drift between “IPC determinism” and “device determinism” requirements.

Normative rules:

- The canonical QEMU harness MUST default to **modern virtio-mmio**:
  - `-global virtio-mmio.force-legacy=off`
- Legacy virtio-mmio MUST be **opt-in** for debugging/bisect only:
  - `QEMU_FORCE_LEGACY=1` enables `-global virtio-mmio.force-legacy=on`
- Proof runs MUST emit an explicit marker proving modern virtio-mmio is active (example marker):
  - `virtio-blk: mmio modern`

Rationale (non-normative):

- Modern virtio-mmio fixes known virtio ring progress issues in QEMU that can otherwise block or flake
  end-to-end proofs (e.g. persistence proofs via virtio-blk).

Implementation reference (non-normative):

- Current canonical implementation lives in `scripts/run-qemu-rv64.sh`.
- Optional (external harness support): `tools/qemu/build-modern.sh` can build a QEMU with a force-modern default,
  but this is not required if the harness global is set.

#### Recommended `nexus-ipc` API surface

This RFC defines the shape; exact module names are task-owned.

- `Nonce`: newtype wrapper over `u64`
- `NonceGen`: monotonic generator (host + os-lite)
- `ReqRepClient`:
  - `send_request_with_nonce(...)`
  - `recv_reply_matching(nonce, deadline) -> ReplyFrame`
  - `call(...) -> ReplyFrame` convenience (send + await)

The dispatcher is intended to be used by:

- `selftest-client` QEMU proof calls
- `statefsd` client calls (`Put/Get/List/Sync`)
- `logd` query flows (APPEND ack, QUERY response)
- `policyd` already uses a nonce in v2/v3; it should adopt the shared dispatcher patterns

### Service protocol adaptations (normative, minimal)

This RFC standardizes nonce usage; each service protocol owns its own magic/op/status.

Minimum adoption rules:

- **policyd**: keep existing v2/v3 nonce semantics; adopt dispatcher for shared inbox use.
- **logd**:
  - **Required for bring-up proofs**: any client that uses `logd` via a shared reply inbox MUST deterministically consume
    and validate the APPEND ACK (bounded loops; no “fire-and-forget”).
  - **LO v2 nonce frames (implemented)**: logd supports a v2 framing that includes a `nonce:u64` so clients can safely
    multiplex multiple in-flight logd RPCs over a shared reply inbox:
    - request: `[MAGIC0, MAGIC1, VERSION(=2), OP, nonce:u64le, ...]`
    - reply:    `[MAGIC0, MAGIC1, VERSION(=2), OP|0x80, status:u8, nonce:u64le, ...]`
- **statefsd**: define `SF v2` frames with nonce for Put/Get/Delete/List/Sync.

Compatibility rule:

- v1 frames MAY be supported concurrently during bring-up, but any flow used for deterministic
  QEMU/CI proofs MUST use nonce-based frames.

### Phases / milestones (contract-level)

- **Phase 0**: Nonce contract + host tests for dispatcher correctness (drop/retain/bounds, out-of-order replies).
- **Phase 1**: Make the logd proof path deterministic under shared inboxes:
  - Clients MUST consume and validate APPEND ACKs deterministically (bounded).
  - Adopt logd nonce frames (`LO v2`) for multiplexed logd RPCs.
- **Phase 2**: Adopt nonce-based frames for statefsd and other high-value control planes (policy checks, update flows), removing ad-hoc drains/yields.

## Security considerations

### Threat model

- **Reply confusion / desync**: attacker/service causes a client to consume the wrong reply (confused deputy).
- **Spoofed identity**: a sender tries to claim another identity via payload.
- **DoS by inbox flooding**: a service floods a shared reply inbox to exhaust memory or delay matching.

### Security invariants (MUST hold)

- **Identity binding**: authorization MUST use kernel-derived `sender_service_id` (or explicit proxy rules),
  not payload fields. Nonce does not change this.
- **No secret leakage**: nonces are non-secret; do not treat them as auth tokens.
- **Bounded dispatcher**: reply buffering is bounded and has deterministic drop behavior.
- **Explicit failure**: timeouts and drops must be surfaced deterministically (tests/markers), never as silent success.

### DON'T DO

- DON'T use nonce as an authentication mechanism.
- DON'T accept caller-provided “service name” strings as identity.
- DON'T build “drain until empty” loops without bounds (risk: hangs/DoS).
- DON'T rely on queue ordering for correctness.

## Failure model (normative)

- **Missing reply**: client times out deterministically (deadline/budget) and surfaces a structured error.
- **Unmatched replies**: stored until matched or dropped due to bounded cap; drop counter increments.
- **Malformed reply**: rejected and counted; MUST NOT be treated as a match.
- **Nonce collision (in-flight)**: forbidden by contract; callers must prevent it. If detected, fail deterministically.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-ipc -- reqrep
```

Required coverage:

- out-of-order replies still match correct nonce
- bounded buffering drops deterministically when cap exceeded
- timeout returns deterministically (no wall-clock sleeps required)

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_PHASE=logd RUN_TIMEOUT=240s just test-os
```

Required markers (examples; task defines exact ladder):

- `SELFTEST: crash report ok`
- `SELFTEST: log query ok`

## Open items (drift guard)

These items are intentionally explicit so we can keep QEMU green without accumulating hidden harness assumptions.

- **Marker ladder proof**: Ensure `scripts/qemu-test.sh` includes and enforces the “modern virtio-mmio active” marker
  (e.g. `virtio-blk: mmio modern`) in the canonical expected ladder when virtio-blk is part of the image set.
- **Harness single entrypoint**: Ensure CI and developer docs consistently invoke QEMU via `scripts/run-qemu-rv64.sh`
  (or enforce the same `virtio-mmio.force-legacy=off` global when using a different wrapper).
- **External harness policy**: If an external harness cannot set QEMU globals, it must either patch QEMU defaults
  (`tools/qemu/build-modern.sh`) or document why it is not supported for deterministic proofs.

## Modernization slices (execution plan; task-owned)

This section is a **modernization list** (tracked as slices) that prevents “half-modern” states where the harness,
IPC correlation, and virtio devices drift apart and reintroduce non-determinism.

The slices MUST be executed **sequentially** for CI determinism. For bring-up we started with **Slice B**
(virtio-net RX correctness) because it was blocking strict DHCP proofs.

### Slice A — IPC correlation everywhere (shared inboxes)

Goal:

- Adopt RFC‑0019 nonce echo + a bounded reply dispatcher **everywhere a shared reply inbox is used**.

Scope (non-exhaustive, but required for CI green):

- `samgrd`, `bundlemgrd`, `updated`, `statefs*`, `logd`, `dsoftbusd` (and any client that multiplexes replies).

Stop condition / proof:

- Deterministic host integration tests for request/reply correlation (out-of-order + cross-talk) for each shared-inbox client.
- QEMU proofs do not rely on “drain stale replies” for correctness; proofs use nonce-based frames.

Status:

- **DONE (for current OS/QEMU proof paths)** (as of 2026-02-06):
  - `userspace/nexus-ipc::reqrep` exists (bounded reply buffer + tests).
  - `dsoftbusd` now buffers unmatched netstackd replies by nonce instead of silently dropping them on the shared inbox.
  - StateFS migrated to nonce-correlated `SF v2` frames (see Phase 2 partial progress).
  - `policyd` delegated-cap checks (`OP_CHECK_CAP_DELEGATED`) have a v2 nonce-correlated form; callers (`statefsd`, `keystored`) use strict nonce matching on the shared reply inbox.
  - `selftest-client` IPC CAP_MOVE + sender attribution probes use nonce-correlated frames (no “drain stale replies” required for correctness).
  - Routing control-plane now supports a backwards-compatible **v1+nonce extension** in init-lite, and key clients use it to avoid stale-drain patterns on ctrl slot 2:
    - `bundlemgrd` (route-status), `rngd`, `statefsd`, `logd` bootstrap routing, and `selftest-client` routing probes.
  - “Log probe” CAP_MOVE paths no longer fill the shared `@reply` inbox:
    - `samgrd`/`bundlemgrd` now wait for and validate the logd APPEND ACK (bounded).
  - `logd` supports `LO v2` nonce frames for multiplexed APPEND/QUERY/STATS, and proof paths use nonce matching on the shared inbox.
  - `rngd` policyd delegated-cap enforcement uses **policyd v2 nonce replies** and strict matching (shared inbox safe).

### Slice B — virtio-net modern (RX/TX path + queue correctness)

Goal:

- Make virtio-net RX/TX progress **correct under modern virtio-mmio** so `REQUIRE_QEMU_DHCP_STRICT=1` becomes stable.

Scope:

- virtqueue setup (modern register programming), QueueReady semantics, RX buffer posting/requeue, notification, and used.idx tracking.
- Ensure driver behavior is deterministic under `-icount` and cooperative scheduling.

Stop condition / proof:

- `RUN_TIMEOUT=240s REQUIRE_QEMU_DHCP=1 REQUIRE_QEMU_DHCP_STRICT=1 just test-os` reaches `net: dhcp bound` and the DHCP-dependent selftests:
  - `SELFTEST: net ping ok`
  - `SELFTEST: net udp dns ok`
  - `SELFTEST: icmp ping ok`
- No temporary RX-probe/selftest harness flags required; the proof is the bound DHCP + dependent selftests.

Known bring-up hazard (runtime-evidenced, Feb 2026; resolved):

- Symptom: DHCP DISCOVER TX looked correct, but Strict DHCP never reached `net: dhcp bound` and higher-level RX proofs (e.g. ping) were missing.
- Root cause: virtio-net RX parsing assumed a **10-byte** `virtio_net_hdr`, but QEMU usernet/virtio-net delivered frames with a **12-byte**
  `virtio_net_hdr_mrg_rxbuf` header (MRG_RXBUF). This misaligned the Ethernet frame by 2 bytes and made RX traffic unreadable to smoltcp.
- Fix:
  - Negotiate `VIRTIO_NET_F_MRG_RXBUF` and derive the virtio-net header length from the **accepted feature set**:
    - 12 bytes (`virtio_net_hdr_mrg_rxbuf`) if MRG_RXBUF accepted
    - 10 bytes otherwise
  - When using the 12-byte header, set `num_buffers=1` for TX.
- Proof: default `REQUIRE_QEMU_DHCP=1 REQUIRE_QEMU_DHCP_STRICT=1 just test-os` now deterministically reaches:
  - `net: dhcp bound ...`
  - `SELFTEST: net ping ok`
  - `SELFTEST: net udp dns ok`
  - `SELFTEST: icmp ping ok`

Status:

- **DONE** (as of 2026-02-06): bring-up diagnostics removed; `just test-os`, `just test-os-dhcp`, `just test-os-dhcp-strict` all green sequentially.

### Slice C — harness/test architecture (DHCP strict policy)

Goal:

- Enforce Strict-DHCP **only** where the QEMU backend guarantees it; otherwise gate on deterministic L3/L4 proofs that do not
  depend on slirp DHCP behavior.

Scope:

- `scripts/qemu-test.sh` policy for `REQUIRE_QEMU_DHCP` vs `REQUIRE_QEMU_DHCP_STRICT`, and documentation under `docs/testing/`.

Stop condition / proof:

- CI uses a single, explicit policy:
  - Strict DHCP runs are only enabled for backends/environments proven to bind deterministically.
  - Non-strict runs accept honest static fallback markers, and skip DHCP-dependent proofs deterministically.

Status:

- **DONE** (as of 2026-02-06): `scripts/qemu-test.sh` enforces `REQUIRE_QEMU_DHCP_STRICT=1` ⇒ **must** see `net: dhcp bound`; non-strict `REQUIRE_QEMU_DHCP=1` accepts the honest fallback marker and skips DHCP-dependent proofs deterministically. Docs updated under `docs/testing/`.

## Affected tasks (drift guard)

Any task that adds or depends on multi-step IPC conversations over shared inboxes MUST reference
this RFC (or a successor) and adopt nonce correlation.

Known tasks impacted by this contract (non-exhaustive):

- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` (explicitly calls out request/conversation IDs)
- `tasks/TASK-0018` Crashdumps v1 (crash artifacts + query/export flows)
- `tasks/TASK-0025` StateFS write-path hardening (budgets + audit + deterministic IPC)
- `tasks/TASK-0040` Remote observability v1 (export/scrape control plane)
- `tasks/TASK-0206-webview-v1_2b-os-history-downloads-resume-csp-ui-recovery.md` (Servo crash reports + proofs)

## Alternatives considered

- **Option A: Dedicated reply endpoint per client**:
  - Pros: simplest matching (no multiplexing)
  - Cons: capability distribution explosion for high fan-in services; more bring-up fragility
- **“Just drain stale replies”**:
  - Reject: not deterministic, breaks under concurrency, encourages brittle workarounds
- **Kernel-managed correlation**:
  - Reject for v1: requires syscall/ABI changes and shifts policy into kernel; not needed for determinism

## Open questions

- Should we standardize a “common envelope” around service frames (generic header + service payload),
  or keep nonce inside each service protocol? (Default: keep per-protocol versioning for v1.)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: nonce + dispatcher contract + host tests — proof: `cargo test -p nexus-ipc -- reqrep`
  - [x] `userspace/nexus-ipc::reqrep`: `NonceGen` + bounded `ReplyBuffer` + unit tests (out-of-order + bounded drops)
  - [x] Add bounded “recv until nonce match” helper (iteration-budget + bounded buffering) + deterministic timeout test
- [x] **Phase 1**: logd proof-path determinism under shared inboxes — proof: `just test-os` (log probes + audit + crash report markers)
  - [x] Clients that use CAP_MOVE to logd consume and validate the APPEND ACK deterministically (bounded), preventing `@reply` buildup.
  - [x] logd nonce frames (`LO v2`) for fully multiplexed logd RPCs — proof: `RUN_PHASE=logd RUN_TIMEOUT=240s just test-os`
- [x] **Phase 2**: statefsd/policyd/execd adopt nonce frames for shared inbox flows — proof: task-defined QEMU markers
  - [x] `statefsd` + `userspace/statefs` client: `SF v2` nonce echo + strict matching on shared `@reply` inbox
  - [x] `policyd` client(s): remove “drain stale replies” by adopting nonce-based matching where shared inbox is used
  - [x] `execd`/others: migrate remaining shared-inbox flows (`execd`, `keystored`, `dsoftbusd`, `selftest-client`, `nexus-init`/`updated` control plane)
