# RFC-0005: Kernel IPC & Capability Model

- Status: In Progress (Phase 0/1 complete; Phase 2 in progress; ABI freeze pending)
- Owners: Runtime + Kernel Team
- Created: 2025-12-18
- Last Updated: 2025-12-22

## Status at a Glance

- **Phase 0 (Kernel IPC v1 + bring-up floor)**: Complete ✅
- **Phase 1 (Bootstrap + proto stabilization)**: Complete ✅
- **Phase 2 (Hardening + lifecycle)**: In progress ✅ (major items implemented; remaining “production-grade” gaps listed below)
- **ABI freeze (compat guarantee)**: Not yet declared (we keep the versioned/descriptor patterns, but do not promise “never change” yet)

Definition:

- In this RFC, “Phase 0 complete” means: kernel IPC v1 syscalls work end-to-end, `userspace/nexus-ipc`
  is wired to the kernel backend, init-lite routing responder works, and the marker-driven QEMU E2E
  run is honest green.

Phase 2 note:

- Endpoint lifecycle is now defined for bring-up: endpoints created via `SYSCALL_IPC_ENDPOINT_CREATE`
  are **owned by the creating task** and are **closed on task exit**, waking any blocked waiters.
  Subsequent operations on closed endpoints fail deterministically (`IpcError::NoSuchEndpoint`).
- Explicit close is supported via two operations:
  - `cap_close(slot)`: drops the caller’s capability slot (local drop).
  - `ipc_endpoint_close(slot)`: requires `Rights::MANAGE` on an endpoint capability and performs a
    **global close** (revocation-by-close), waking blocked waiters.

## What’s still missing (to call RFC‑0005 “Complete”)

This section lists the remaining work to go from “bring-up complete” to “production-grade complete”.
Items here should be backed by either kernel selftests, QEMU markers, or unit tests.

### Phase 2 (Hardening) — remaining gaps

- **Capability lifecycle**
  - [ ] Cap revocation beyond “close endpoint”: define and implement a general revocation story (if needed), or explicitly scope it out
  - [ ] Clarify/lock “cap table full” behavior across all syscalls that allocate caps (clone/recv/CAP_MOVE)

- **Identity binding completeness**
  - [ ] Apply channel-bound identity checks to all security-critical service protocols (eliminate “trust requester string/id” patterns everywhere)
  - [ ] Document (and/or enforce) init-lite “proxy authority” rules as a first-class concept (not ad-hoc service-name allowlist)

- **IPC production-grade**
  - [ ] Stress/soak + fuzz: randomized send/recv/CAP_MOVE/close/exit sequences (no deadlocks, no leaks, no starvation)
  - [ ] Fairness policy documented (FIFO is implemented; define starvation bounds or QoS behavior)

### ABI freeze (compat guarantee) — required before “Complete”

- **Syscall ABI**
  - [ ] Publish a stable ABI contract: what is frozen, what is versioned, and what can evolve
  - [ ] Golden vectors / compatibility tests for all on-wire frames we claim stable (routing v*, policyd v*, execd v1, bundleimg v1)

- **Userspace API**
  - [ ] “Blessed” request/reply patterns (ReplyCap / CAP_MOVE) documented as the recommended style, with service migrations tracked

## Context

RFC-0002 establishes process-per-service isolation and RFC-0004 hardens the loader and memory
provenance. Kernel IPC endpoint routing exists, IPC v1 payload syscalls are available, and the
os-lite bring-up path is now wired cross-process via `userspace/nexus-ipc`'s kernel backend plus an
init-lite routing responder.

This RFC should be read alongside the project vision lens (Rust-first, RISC‑V-first, HarmonyOS-like
device mesh via `softbusd` layering):

- `docs/agents/VISION.md`

As the system grows, keeping IPC and capability semantics embedded across RFC-0002/RFC-0004 makes
those RFCs too large and encourages drift. We need one stable, explicit contract for:

- What capabilities exist (endpoint, VMO, etc).
- How rights are enforced and transferred.
- How IPC is performed (syscalls/ABI, blocking semantics, error model).
- How services are bootstrapped with initial caps and identity.

## Goals

1. Define a kernel-enforced IPC model based on endpoint capabilities with explicit rights.
2. Define a capability transfer/derivation model that supports least privilege.
3. Define a stable syscall ABI for IPC and capability operations required by userland services.
4. Define the bootstrap protocol between `init-lite` and services (service identity + initial caps).
5. Keep the design compatible with RFC-0002 (process-per-service) and RFC-0004 (loader safety).

## Non-Goals

- A full policy language (handled by `policyd` once IPC exists).
- A full service manager design (handled by `samgrd` once IPC exists).
- A generalized logging control plane (RFC-0003).
- Long-term distributed IPC (out of scope; see distributed docs).

## Roadmap (how we continue from here)

We implement RFC‑0005 in small, proofable increments. The guiding dependency chain is:

- **RFC‑0004 (security floor)**: pointer provenance + W^X + deterministic cleanup guarantees that
  IPC syscalls can safely copy in/out of user memory without implicit trust.
- **RFC‑0003 (logging)**: keep logs/markers deterministic and provenance-safe so IPC/policy failures
  are debuggable without adding “always-on” noisy infrastructure.

Next focus (Phase 2 hardening):

- **Ownership correctness**: anything created “for service X” should be owned by X (close-on-exit).
  Bring-up now supports `ipc_endpoint_create_for(factory, owner_pid, depth)` for this purpose.
  Init-lite uses this for **service request endpoints** so the receiver service owns the endpoint
  lifetime even when init-lite performs the initial capability distribution.
- **Per-requester endpoint pairs**: init-lite should not reuse a single endpoint pair for multiple
  requesters. Each requester→service link gets its own request/response endpoints, with ownership
  set to the receiver side for correct close-on-exit semantics (e.g. bundlemgrd→execd uses a
  dedicated pair, not the selftest-client↔execd channels).
- **Authority rule for `create_for`**: even with an `EndpointFactory`, a task may only create
  endpoints owned by **itself** or by one of its **direct children** (init-lite → spawned services).
- **Lifecycle tightening**: ensure closed endpoints and task exit wake any blocked peers and never
  leave waiters stuck; add negative tests for disconnected paths.
- **Authority tightening**: Endpoint creation remains authorized by `EndpointFactory`; the next
  step is to make factory distribution policy-controlled (policyd/samgrd), replacing “bootstrap is
  special” scaffolding over time.
- **Factory distribution rule (current hardening)**: `EndpointFactory` is not generally transferable.
  Only bootstrap (PID 0) may transfer it to init-lite (PID 1). Services do not receive factory caps.

## Decision

Adopt a **capability-based kernel IPC** design where all IPC operations require an **Endpoint**
capability and explicit rights. Remove reliance on the os-lite mailbox registry for security; it
may remain as a temporary bring-up backend but is not security-relevant.

## Capability Types

This RFC defines the minimum set of capability kinds required to de-stub core services.

- **Endpoint**
  - Represents a message queue owned by a receiver task.
  - Rights: SEND, RECV, GRANT (optional, see below).
- **VMO** (already exists)
  - Represents a kernel-managed memory object.
  - Rights: MAP (and future READ/WRITE variants if needed).
- **AddressSpace handle** (already exists)
  - Not a cap itself today; if promoted to a cap later, update this RFC.

## Rights Model

### Endpoint rights

- **SEND**: caller may enqueue a message to the endpoint.
- **RECV**: caller may dequeue messages from the endpoint.

Notes:

- The current ABI (`nexus-abi::Rights`) defines **SEND**, **RECV**, **MAP**, **MANAGE**.
- This RFC reserves the concept of “attach caps to messages” for a later extension; do not assume
  it exists today. Capability passing is currently performed explicitly via `cap_transfer`.

Rules:

- Rights are always a subset when transferring/deriving.
- The kernel enforces rights at syscall boundaries; userland must not rely on conventions.

## IPC Message Model

### Relationship to our existing “IDL + filebuffer/VMO” hybrid

This RFC is designed to fit the architecture we already have today:

- **Control plane**: Cap’n Proto frames produced/consumed by `userspace/nexus-idl-runtime`
  (see `docs/adr/0004-idl-runtime-architecture.md` and `tools/nexus-idl/schemas/*.capnp`).
- **Data plane**: large payloads (bundle artifacts, file contents, etc.) travel out-of-band via
  **VMO-backed buffers** and are referenced from the Cap’n Proto control message by a handle id
  (e.g. `vmoHandle :UInt32` in `bundlemgr.capnp`).

This is deliberately similar in spirit to **Fuchsia channels + VMOs** (small typed messages plus
separate transferable memory objects), and compatible with seL4’s “everything is capabilities”
discipline, while staying ergonomic for Rust.

Key invariants:

- **No raw pointers in IPC payloads.** Any buffer/data reference must be by capability/handle id.

### Capability passing (Phase 2 / bring-up scalability)

To avoid hard-coding per-client reply channels, IPC v1 supports moving **one capability** alongside
an IPC message:

- Sender sets `nexus_abi::ipc_hdr::CAP_MOVE` in `MsgHeader.flags` and places the capability slot to
  move into `MsgHeader.src`.
- Kernel **consumes** that cap slot on send, carries it in the queued message, and on receive
  **allocates** it into the receiver cap table.
- On receive, `MsgHeader.src` is overwritten with the newly allocated receiver slot.

Security floor:

- In-band cap move does not permit moving `Rights::MANAGE` authority (and never allows moving the
  `EndpointFactory`).
- CAP_MOVE supports non-blocking, blocking, and timeout send semantics. The kernel MUST ensure the
  moved capability is only consumed on a successful enqueue and MUST be rolled back on failure so
  syscall retry/reschedule never “loses” the cap.
- If the receiver cannot allocate a capability slot for a moved cap, the receive MUST fail with
  `ENOSPC` (`IpcError::NoSpace`) and the message MUST NOT be lost (it remains queued).

Bring-up routing note:

- Init-lite exposes a requester-local reply inbox via routing name **`@reply`**, returning
  `(send_slot, recv_slot)` for a capability pair referencing the same endpoint (SEND vs RECV).
  Clients can CAP_MOVE the SEND cap to a server and then receive the reply on the RECV cap.

Related primitive:

- `cap_clone(slot)` duplicates a cap locally. This allows clients to keep a long-lived SEND handle
  while CAP_MOVE-ing short-lived clones for individual requests.

Example (samgrd RPC style):

- Client resolves `@reply` to get `(reply_send, reply_recv)`.
- For each request: `cap_clone(reply_send)` → CAP_MOVE the clone to `samgrd` → receive the reply on
  `reply_recv`.

Recommended userspace pattern:

- Servers should prefer `recv_request()` (frame + optional one-shot `ReplyCap`) and, when present,
  reply via that cap and close it. This keeps reply routing explicit, avoids per-client server-side
  state, and composes cleanly with `@reply` for bring-up.

Userspace convenience API note:

- `userspace/nexus-ipc` kernel backend exposes `send_with_cap_move_wait(...)` and
  `ReplyCap::reply_and_close_wait(...)` so CAP_MOVE can use blocking/timeout waits when needed.
- **“Handle ids” carried in IDL are capability slot indices.** On OS builds, a `vmoHandle` value
  is the integer capability slot for a VMO-capability in the sender’s task; it is only meaningful
  if the receiver also has (or is granted) the corresponding capability.
- **Mapping is capability-gated.** The receiver may only map/use a VMO if it holds a VMO capability
  with `Rights::MAP` (and later READ/WRITE sub-rights if we split `MAP`).
- **Provenance + W^X apply.** Any shared-memory strategy must obey RFC‑0004; the default data-plane
  is “VMO capability + explicit map”, never RWX and never reusing stale bootstrap scratch pages.

### Glossary (normative, OS build)

We use a “capability + IDL metadata” hybrid. These terms MUST be used consistently in kernel and
userspace APIs to avoid drift:

- **Capability slot (slot index)**: an integer index into a task’s capability table. In the kernel
  this is `SlotIndex`; in userland this commonly appears as a `u32`.
- **Capability (cap)**: a capability *reference* held in a slot (e.g. Endpoint cap, VMO cap).
  Rights are attached to the cap and checked by the kernel.
- **EndpointId**: the router’s internal endpoint identifier (`u32`). Userland MUST NOT treat this
  as authority; authority is the capability slot (capability) not the endpoint id.
- **VMO handle (IDL)**: a `UInt32` field in Cap’n Proto schemas (e.g. `vmoHandle`). In OS builds,
  this value is a **capability slot index** referring to a VMO capability. On host builds it may be
  emulated (see tests), but the OS meaning is normative.
- **nexus_abi::Handle**: the current userland type alias used for VMOs. In today’s tree it behaves
  like a slot-id. This RFC treats it as “slot index carrying a VMO capability”.

Corollary: if an IDL message contains a `vmoHandle` but the receiver does not hold the
corresponding capability (or did not receive it via transfer), the handle MUST be treated as
invalid and the request MUST fail deterministically.

### Control-plane + data-plane protocol (normative)

This is the standard pattern for “IDL references bulk bytes”:

1. **Producer** allocates a VMO and writes bytes to it.
2. Producer constructs the Cap’n Proto request carrying:
   - `vmoHandle` (slot id in producer’s cap table)
   - length/offset metadata (`bytesLen`, etc.)
3. Producer ensures the **consumer holds the VMO capability**:
   - **Today**: via `cap_transfer` (explicit transfer), or by prior bootstrap distribution.
   - **Future**: may be integrated as “handles attached to messages” (Fuchsia-style), but only if
     it stays capability-safe and Rust-friendly.
4. Producer sends the Cap’n Proto frame via endpoint IPC.
5. **Consumer** validates:
   - the referenced slot exists and is a VMO capability
   - it has required rights (`Rights::MAP`)
   - length/offset are within bounds
6. Consumer maps/reads the VMO and completes the operation.
7. Consumer drops/returns the capability per service policy (revocation/close rules are a follow-up).

This protocol is intentionally simple and explicit: it reuses the existing cap system and matches
our current OHOS-like “service graph” approach (RFC‑0002) without importing seL4/Fuchsia mechanisms
that do not fit our Rust/no_std constraints.

### OHOS alignment + future `softbusd` note

This RFC focuses on **local (same-kernel) IPC**. It is aligned with the OHOS-style architecture
we’re building: services communicate via a small, typed control plane (IDL) and a separate bulk
data plane (VMO/filebuffer).

Future distributed messaging (e.g. a `softbusd` service) MUST be layered above this model:

- `softbusd` becomes “just another service” in the capability graph.
- Local services talk to `softbusd` via the same endpoint IPC + IDL framing.
- Cross-node transport, discovery, and crypto are handled in `softbusd` userland; the kernel ABI
  defined here should not bake in network/distributed assumptions.

### Bootstrap capability graph (recommended, initial OS bring-up)

This is a *recommended* initial distribution. It is intentionally simple and matches our current
service architecture (ADR‑0017) and policy architecture (ADR‑0014). The precise set will evolve as
`policyd` and `samgrd` become fully authoritative.

Principles:

- **init-lite is a temporary root-of-authority** for early boot only.
- **samgrd becomes the service discovery and endpoint distributor**.
- **policyd becomes the capability gatekeeper** for privileged operations and future cap distribution.
- **keystored/identityd are roots for device identity / secrets** and should be minimally trusted.

Legend:

- `EP(service)` = endpoint capability for contacting `service`
- Rights: `SEND`, `RECV` on endpoints; `MAP` on VMOs

#### Stage A: init-lite bootstraps core authority

- **init-lite**
  - Holds: ability to `exec` services; bootstrap endpoints; minimal debug UART.
  - Gives to `samgrd`: `EP(init-lite)` (bootstrap channel), rights `SEND|RECV`
  - Gives to `policyd`: `EP(init-lite)` (bootstrap channel), rights `SEND|RECV`
  - Gives to each spawned service: `EP(init-lite)` (bootstrap channel), rights `SEND|RECV`

Rationale: every service can at least report readiness/failure to init-lite and request its initial
cap set through a single well-known bootstrap path.

#### Stage B: samgrd provides discovery + service endpoints

- **samgrd**
  - Receives from init-lite: bootstrap endpoint to init-lite.
  - Owns: registry of named services → endpoints.
  - Provides to clients (per policy): `EP(target_service)` with `SEND` (and `RECV` if the protocol is bidirectional).

Rationale: OHOS-like “system ability manager” role, but with explicit caps instead of ambient names.

#### Stage C: policyd enforces capability decisions

- **policyd**
  - Receives from init-lite: bootstrap endpoint to init-lite.
  - Owns: policy DB (TOML) and the decision procedure.
  - Receives from samgrd: service registration events / identity claims (future tightening).
  - Provides: allow/deny decisions; may later authorize cap transfer/derivation flows.

Rationale: keep policy logic out of the kernel; kernel only enforces rights on already-held caps.

#### Recommended initial per-service caps (coarse, to be tightened)

These are intentionally coarse “first real system” defaults; `policyd` should later shrink them.

- **keystored**
  - Needs: `EP(policyd)` (to ask authorization), `EP(samgrd)` (register), `EP(init-lite)` (bootstrap)
  - Avoid: receiving arbitrary VMOs unless explicitly required (secrets stay internal)

- **identityd**
  - Needs: `EP(keystored)` (keys), `EP(policyd)`, `EP(samgrd)`, `EP(init-lite)`
  - Future: will be a dependency for `softbusd` authentication, but stays local-only for now

- **bundlemgrd**
  - Needs: `EP(packagefsd)` (fetch bundle files), `EP(vfsd)` (file IO), `EP(policyd)` (cap checks),
    `EP(samgrd)` (discovery), `EP(init-lite)` (bootstrap)
  - For large artifacts: VMO caps with `MAP` (data-plane), never inline megabytes

- **packagefsd**
  - Needs: `EP(vfsd)`, `EP(policyd)`, `EP(samgrd)`, `EP(init-lite)`
  - Uses: VMO/filebuffer for package payloads

- **vfsd**
  - Needs: `EP(policyd)`, `EP(samgrd)`, `EP(init-lite)`
  - Provides: file operations; may receive VMO/filebuffer capabilities for reads/writes

- **execd**
  - Needs: `EP(policyd)` (authorize exec/spawn), `EP(samgrd)`, `EP(init-lite)`
  - Optional: `EP(bundlemgrd)` (resolve package->binary)

- **resmgrd** (later)
  - Needs: `EP(policyd)`, `EP(samgrd)`, `EP(init-lite)`
  - Role: resource governance; does not need broad VMO rights by default

- **softbusd** (future, not implemented here)
  - Needs (local): `EP(identityd)` (device identity), `EP(keystored)` (keys), `EP(policyd)` (authorization),
    `EP(samgrd)` (discover routes), `EP(init-lite)` (bootstrap)
  - Data-plane: uses explicit buffer/VMO handoffs for payload staging; network transport is userland.

This table is intentionally a starting point. Once the kernel IPC transport v1 exists and policyd
is real, we tighten the default by making endpoint distribution come only from samgrd+policyd
instead of init-lite.

### Message payload

Minimum viable message:

- A byte payload (bounded by `MAX_FRAME_BYTES`).
- Optional attached capability transfers (bounded by `MAX_CAP_XFERS`).

In our system, the byte payload is typically one of:

- A Cap’n Proto-encoded request/response frame (IDL runtime).
- A small fixed-format header for bring-up/selftests.

Large payloads MUST NOT be sent inline once the VMO/filebuffer hybrid is available; instead, send
metadata inline and reference the bulk bytes via a VMO capability.

### Bulk buffer pattern (recommended)

For “big bytes”, use the following pattern:

1. Producer allocates a VMO and writes bytes into it.
2. Producer includes the VMO slot id (e.g. `vmoHandle`) + `bytesLen`/offset metadata in the IDL
   message.
3. Producer transfers the VMO capability to the consumer (today: via `cap_transfer`; future: may be
   integrated into message passing).
4. Consumer maps the VMO with `Rights::MAP` and consumes the bytes.
5. Consumer closes/drops its capability when done (explicit cap close/revocation semantics are a follow-up;
   endpoint close-on-exit exists as a minimal lifecycle rule).

### Blocking semantics

Syscalls MUST support:

- Non-blocking (`WouldBlock`)
- Blocking (`Blocking`)
- Timeout (`Timeout`)

Blocking MUST yield scheduler ownership in-kernel (no busy loops in userland for the secure path).

### Backpressure

- Send to a full queue returns `QueueFull` (or blocks if requested).
- Recv from an empty queue returns `QueueEmpty` (or blocks if requested).

Queue depth is enforced per endpoint (created with `ipc_endpoint_create_v2/_for(..., depth)` and clamped to 1..256):

- If the endpoint queue is full:
  - **Non-blocking send** fails with `EAGAIN` (`IpcError::QueueFull`).
  - **Blocking/timeout send** blocks the sender (wait-queue) until space is available or deadline
    expires (`ETIMEDOUT` / `IpcError::TimedOut`).

### Resource exhaustion (DoS hardening)

- Endpoint creation is bounded (router quota). If the kernel cannot allocate a new endpoint, endpoint
  creation returns `ENOSPC` (`IpcError::NoSpace`).
- Endpoint creation is also bounded per owner PID (per-owner quota) to prevent a single compromised
  service from exhausting the global endpoint table.

Blocking correctness proof (wait queues):

- `KSELFTEST: ipc send unblock ok`
- `KSELFTEST: ipc close wakes ok`
- `KSELFTEST: ipc owner-exit wakes ok`

What is still missing for “production grade” blocking:

- **Fairness/starvation**: define ordering guarantees for waiter queues; avoid priority inversions.
- **Accounting**: memory/bytes accounting per endpoint (not only queue depth), plus stable OOM/ENOSPC behavior.
- **Soak/fuzz**: long-running randomized scheduling tests + adversarial patterns (close/exit races, heavy CAP_MOVE).

Queued-bytes accounting (DoS hardening):

- In addition to **queue depth**, each endpoint has a bounded **queued-bytes budget**.
- If a send would exceed this budget, it fails with **`ENOSPC` / `IpcError::NoSpace`** (deterministic).
- Depth exhaustion remains **`EAGAIN` / `IpcError::QueueFull`**.
- The router also enforces a **global queued-bytes budget** across all endpoints to bound total memory use.
- The router also enforces a **per-owner queued-bytes budget** across endpoints owned by the same PID (service inbox cap).

Proof:

- `KSELFTEST: ipc bytes full ok`
- `KSELFTEST: ipc global bytes budget ok`
- `KSELFTEST: ipc owner bytes budget ok`

Wait-queue fairness (current scope):

- Recv waiters are woken in **FIFO order** (registration order).
- Send waiters are woken in **FIFO order** (registration order).

Proof:

- `KSELFTEST: ipc recv waiter fifo ok`
- `KSELFTEST: ipc send waiter fifo ok`

## Syscall ABI (proposed)

This RFC uses the **current kernel syscall IDs** and register layout as implemented in
`source/kernel/neuron/src/syscall/mod.rs` and `source/kernel/neuron/src/syscall/api.rs`.

### Register conventions

- RISC-V syscall arguments are passed in **a0–a5**; return value is in **a0**.

### Error encoding (Rust-friendly)

Inspiration:

- **seL4**: syscalls return explicit error codes; userland does not rely on “task killed on error”.
- **Fuchsia (Zircon)**: syscalls return a status; success returns data, failure returns a code.

Contract:

- IPC syscalls MUST return errors to userspace as **negative errno** values in `a0`.
- IPC syscalls MUST NOT terminate the calling task for normal, expected errors (permission denied,
  queue empty/full, invalid args, timeout). (Killing tasks on error is a bring-up convenience, not
  a stable ABI.)
- Userland wrappers MUST interpret `a0` as `isize`:
  - `a0 >= 0` → success
  - `a0 < 0` → failure with `errno = -a0`

Mapping to `nexus_abi::IpcError` (stable):

- `EPERM (1)` → `IpcError::PermissionDenied`
- `ESRCH (3)` → `IpcError::NoSuchEndpoint` (also used for “disconnected/closed endpoint” cases)
- `EAGAIN (11)` → `IpcError::{QueueEmpty|QueueFull}` depending on operation
- `ENOSPC (28)` → `IpcError::NoSpace` (e.g. receiver cannot allocate CAP_MOVE capability slot)
- `ETIMEDOUT (110)` → `IpcError::TimedOut`
- `ENOSYS (38)` → `IpcError::Unsupported`
- Anything else → `IpcError::Unsupported` (until extended)

Note: `ENOSPC` may also be returned by capability-management syscalls (e.g. `cap_clone`) when the
current task has no free capability slots (bounded cap table hardening).

### Kernel IPC syscalls (current)

#### `SYSCALL_SEND = 2` (header-only v0)

- **Args**:
  - a0: `slot` (capability slot index)
  - a1: `ty` (u16)
  - a2: `flags` (u16)
  - a3: `len` (u32)
- **Returns**: `len` on success
- **Current implementation status**:
  - Rights checked: requires `Rights::SEND`
  - Payload: currently **empty** (no user copy-in yet)

#### `SYSCALL_RECV = 3` (header-only v0)

- **Args**:
  - a0: `slot` (capability slot index)
- **Returns**: `len` on success
- **Current implementation status**:
  - Rights checked: requires `Rights::RECV`
  - Payload/header copy-out: **not yet exposed** to userspace (kernel keeps last message for tests)

### Capability transfer syscall (current)

#### `SYSCALL_CAP_TRANSFER = 8`

Transfers one capability from the current task into a child task’s capability space.

- **Args**:
  - a0: `child` (pid)
  - a1: `parent_slot` (cap slot in current task)
  - a2: `rights_bits` (subset of `nexus-abi::Rights`)
- **Returns**: destination slot index in the child on success

### Planned IPC syscalls (v1: payload copy-in/out)

To reach “real services with real security”, we need a copy-in/out transport. The preferred path is
to add explicit v1 syscalls (keeping the v0 IDs stable for bring-up).

#### Syscall IDs (v1)

This RFC reserves syscall IDs for v1 (chosen to avoid conflicts with existing IDs in the tree):

- `SYSCALL_IPC_SEND_V1 = 14`
- `SYSCALL_IPC_RECV_V1 = 18`

#### Message header layout

Userland uses `nexus_abi::MsgHeader` (16 bytes, little-endian):

- `src: u32` (cap slot index used by the sender)
- `dst: u32` (endpoint id; informational for recv, ignored on send)
- `ty: u16` (opcode / message label)
- `flags: u16` (message flags, separate from syscall flags)
- `len: u32` (payload length in bytes)

Kernel behavior:

- On **send**, the kernel MUST derive the destination endpoint from the capability referenced by
  `slot` and MUST overwrite header `src` and `dst` with the authoritative values (do not trust
  userland-provided `src`/`dst`).
- On **recv**, the kernel MUST write back a fully-populated header to userspace.

#### `SYSCALL_IPC_SEND_V1` (copy-in)

- **Args**:
  - a0: `slot` (capability slot index; must have `Rights::SEND`)
  - a1: `header_ptr` (user pointer to `MsgHeader`, 16 bytes)
  - a2: `payload_ptr` (user pointer to payload; may be 0 when `len==0`)
  - a3: `payload_len` (bytes; must equal `header.len`)
  - a4: `sys_flags` (see below)
  - a5: `deadline_ns` (absolute time; 0 means “no deadline”; used when blocking/timeout is requested)
- **Returns**:
  - `>= 0`: number of bytes enqueued (equals `payload_len`)
  - `< 0`: negative errno (see mapping above)

Validation rules (decode/check/execute, seL4-style):

- User pointers MUST be validated (`header_ptr..+16`, `payload_ptr..+payload_len`) against the
  user address limit and must not overflow.
- `payload_len` MUST equal `header.len`.
- `payload_len` MUST be bounded by `MAX_FRAME_BYTES` (initially 512; may be increased later).
- Rights MUST be enforced via the capability slot (`Rights::SEND`).

Blocking semantics:

- If queue is full and `IPC_SYS_NONBLOCK` is set → return `-EAGAIN`.
- If queue is full and blocking is requested → the kernel blocks until space is available or
  `deadline_ns` expires (then return `-ETIMEDOUT`).

#### `SYSCALL_IPC_RECV_V1` (copy-out)

- **Args**:
  - a0: `slot` (capability slot index; must have `Rights::RECV`)
  - a1: `header_out_ptr` (user pointer to `MsgHeader`, 16 bytes)
  - a2: `payload_out_ptr` (user pointer to buffer)
  - a3: `payload_out_max` (buffer size in bytes)
  - a4: `sys_flags`
  - a5: `deadline_ns` (absolute time; 0 means “no deadline”)
- **Returns**:
  - `>= 0`: number of payload bytes written
  - `< 0`: negative errno

Validation rules:

- `header_out_ptr` MUST be writable (user range) for 16 bytes.
- If a message is received with `len > payload_out_max`, behavior is controlled by `sys_flags`:
  - If `IPC_SYS_TRUNCATE` is set: write `payload_out_max` bytes and set the header’s `len` to the
    **original** message length, allowing the caller to detect truncation.
  - Otherwise return `-EINVAL` and leave buffers unmodified.
- Rights MUST be enforced via the capability slot (`Rights::RECV`).

Blocking semantics:

- If queue is empty and `IPC_SYS_NONBLOCK` is set → return `-EAGAIN`.
- If queue is empty and blocking is requested → block until a message arrives or deadline expires
  (return `-ETIMEDOUT`).

#### Syscall flag bits (`sys_flags`)

Bit layout is stable:

- `IPC_SYS_NONBLOCK = 1 << 0`
- `IPC_SYS_TRUNCATE = 1 << 1` (recv only)

The exact v1 signature is a follow-up change under this RFC; the milestone acceptance tests below
define the required user-visible behavior.

Note: this RFC does not mandate zero-copy yet; copy-in/out is acceptable initially, but must
remain compatible with RFC-0004 provenance and W^X constraints.

## Bootstrap Protocol (init-lite → service)

On service spawn, the service MUST receive:

1. A **bootstrap endpoint** capability (slot well-known, e.g. slot 0) that allows it to speak to
   a bootstrap responder in init (or samgrd later).
2. A **service identity token** (string or numeric ID) so IPC can be bound to a name without
   trusting userland globals.

Implementation note (current OS bring-up):

- The kernel seeds the child cap table with the parent's bootstrap endpoint in **slot 0** at
  task creation time. Init must treat slot 0 as reserved and distribute per-service endpoints
  in deterministic subsequent slots (e.g. slot 1 = request, slot 2 = reply for VFS).
- `cap_transfer` remains the mechanism for later stages where init (or samgrd) hands out
  additional endpoints or right-filtered capabilities; it must never amplify rights.

### Bootstrap Routing (init-lite responder, RFC-0005 bring-up)

To avoid hard-coding capability slot numbers in services, init-lite provides a minimal routing
responder that answers "what slots should I use to talk to service X?" queries.

Current protocol (v1, bring-up):

- Init-lite transfers two private "control" endpoint capabilities into every service:
  - **slot 1**: control **REQ** endpoint (**SEND**). Child sends route queries to init-lite.
  - **slot 2**: control **RSP** endpoint (**RECV**). Child receives route replies from init-lite.

Rights note:

- Init-lite transfers **SEND-only** / **RECV-only** endpoint capabilities whenever possible (instead
  of ambient SEND|RECV), so services cannot accidentally use the reverse direction of a channel.
- `Rights::MANAGE` is treated as **close authority** for endpoints. For the current security floor,
  `MANAGE` is **not transferable via `cap_transfer`**; global close must come from the original
  holder (or future explicit policy-controlled distribution).

Endpoint factory (Phase 2 hardening):

- Endpoint creation is authorized via an explicit **EndpointFactory** capability. Init-lite holds
  this capability during bring-up and uses `ipc_endpoint_create_v2(factory_slot, depth)` to mint
  endpoints.
- As a security floor, `Rights::MANAGE` remains non-transferable **except** for transferring the
  EndpointFactory capability into init-lite.
- Current bring-up mechanism: the kernel injects `EndpointFactory(MANAGE)` into init-lite when the
  bootstrap task (PID 0) spawns the init-lite userspace task. Concretely: bootstrap carries the
  factory in cap **slot 2**, and the kernel places a derived copy into init-lite cap **slot 1**
  (where init-lite expects it for `ipc_endpoint_create_v2`). This is temporary scaffolding until
  samgrd/policyd become authoritative distributors.
- Endpoint ownership during bring-up: init-lite can create control endpoints owned by a spawned
  service PID via `ipc_endpoint_create_for(factory, owner_pid, depth)` so that close-on-exit cleanup
  follows the service lifetime (even though init-lite retains the creator cap for distribution).
- Route query frame (child → init-lite, sent on control slot 1):
  - Bytes 0..2: magic `RT` (`0x52,0x54`)
  - Byte 2: `version = 1`
  - Byte 3: `OP_ROUTE_GET = 0x40`
  - Byte 4: `name_len` (u8)
  - Bytes 5..: UTF-8 service name bytes
- Route reply frame (init-lite → child, sent on the control reply endpoint, received on slot 2):
  - Bytes 0..2: magic `RT` (`0x52,0x54`)
  - Byte 2: `version = 1`
  - Byte 3: `OP_ROUTE_RSP = 0x41`
  - Byte 4: `status` (`0` = OK, `1` = NOT_FOUND, `2` = MALFORMED, `3` = DENIED)
  - Bytes 5..9: `send_slot` (u32 LE)
  - Bytes 9..13: `recv_slot` (u32 LE)

Implementation note:

- The encode/decode helpers live in `nexus-abi::routing` so init-lite and `userspace/nexus-ipc`
  share one routing frame contract.

Security note:

- The control endpoints are per-process and not shared between services, preventing ambient
  discovery. Init-lite remains the authority that decides which endpoints (and rights) each
  service receives.
- In bring-up, init-lite may consult `policyd` to **deny routing** for specific (requester,target)
  pairs and respond with `STATUS_DENIED` instead of returning capability slots.

Bring-up proof note:

- The `bundlemgrd -> execd` denial used by the QEMU selftest is **policyd-gated** (not hardcoded in
  init-lite). The underlying route is otherwise present, so the observed `STATUS_DENIED` is a real
  policy decision rather than a `STATUS_NOT_FOUND`.

### Bundle Image (bring-up)

To move VFS from “embedded bytes” to a realistic packaging flow, `bundlemgrd` serves a small
read-only **bundle image** to `packagefsd`, which then serves files to `vfsd` over IPC.

- **Image format**: `NXBI` v1 (see `nexus_abi::bundleimg`) containing a list of entries:
  `(bundle, version, path, kind, data)`.
- **Transport**: `bundlemgrd` exposes `OP_FETCH_IMAGE` (see `nexus_abi::bundlemgrd`), returning the
  raw image bytes.
- **Reply correctness**: `bundlemgrd` supports **CAP_MOVE reply caps** for request/reply so multiple
  clients do not share a single fixed reply queue.

Bring-up proof:

- `SELFTEST: bundlemgrd v1 image ok`

### Service Identity Token (bring-up)

To avoid trusting user-supplied requester strings for security-sensitive decisions, the kernel
derives a stable numeric **ServiceId** from the service name provided to `exec_v2` and publishes it
to the child via the read-only `BootstrapInfo` page:

- `BootstrapInfo.version = 2`
- `BootstrapInfo.service_id: u64` (FNV‑1a 64 of the service name bytes)

Policy upgrade (policyd v3):

- Init-lite uses ID-based policyd control frames (v3) so routing/exec authorization checks do not
  depend on userland-provided requester strings.
- Note: fully binding identity to IPC senders (so servers can attribute messages to a specific
  requester without out-of-band data) is still a follow-up item.

Sender attribution (bring-up):

- Kernel IPC v1 writes the **sender PID** into `MsgHeader.dst` on receive.
- This is a transitional mechanism until we bind service identity directly to IPC senders.

Sender identity binding (bring-up):

- Kernel binds a **service_id** to each task at `exec_v2` time (see `BootstrapInfo.service_id`).
- IPC recv v2 (`ipc_recv_v2`) additionally returns `sender_service_id` (u64) via out-parameter.

Implementation note (policyd control frames):

- Init-lite ↔ `policyd` uses **nonce-correlated v2 frames** for `OP_ROUTE` and `OP_EXEC` so replies
  can be matched to requests without “drain stale replies” hacks. The legacy v1 policyd service
  frames remain for bring-up compatibility (e.g. selftest `OP_CHECK`).

Initial minimal policy:

- init-lite holds the authority to create endpoints and distribute them.
- Later, init-lite delegates policy decisions to `policyd` (see Migration).

## Security Considerations (RFC-0002/0004 alignment)

- No ambient authority: services only interact via caps they are explicitly given.
- W^X and provenance: any shared-memory IPC must obey RFC-0004 (no RWX mappings; provenance tracked).
- Crash containment: faults in one service must not compromise others.
- Capability revocation is not required for MVP; document that it is best-effort or absent.

## Migration Plan

### Stage 0 (bring-up compatibility)

- Keep os-lite mailbox for local bring-up, but treat it as **non-secure**.
- Ensure all marker-driven selftests can pass without relying on mailbox correctness.

### Stage 1 (kernel IPC MVP)

- Lock down the syscall ABI as “stable enough”:
  - Syscall IDs for IPC/cap transfer are stable (`SEND=2`, `RECV=3`, `CAP_TRANSFER=8`).
  - Rights bit meanings match `nexus-abi::Rights`.
- Implement kernel IPC transport v1 (copy-in/out), keeping W^X and user-slice validation.
- Port `nexus-ipc` OS backend (non-os-lite) to use the kernel syscall transport.

Notes (blocking semantics):

- Blocking `ipc_send_v1` / `ipc_recv_v1` and `wait` are implemented as **reschedule + retry**.
  The syscall handler may request an immediate reschedule without advancing `sepc`, and the
  syscall is retried when the task runs again. This avoids switching `current_pid` inside a
  syscall handler without also switching SATP, which would otherwise run code in the wrong
  address space.
- This is still **not** a full sleep/wakeup wait-queue; it is a minimal, deterministic bring-up
  mechanism until proper blocking primitives land.

### Stage 2 (unstub core services)

Order recommended:

1. `keystored` (root of trust)
2. `policyd` (authorization)
3. `packagefsd`
4. `bundlemgrd`
5. `vfsd`
6. `samgrd`
7. `execd`
8. `identityd` (once capnp/no_std story is settled)

## Testing

### Milestones + acceptance criteria

#### M0: boot stability (already achieved)

- `RUN_UNTIL_MARKER=1` QEMU run exits successfully (exit code 0) on expected UART markers.
- User page faults terminate the faulting task instead of fault-storming indefinitely.

#### M1: kernel IPC transport v1 (required)

Acceptance:

- A userspace client can `send` a payload and the server can `recv` the same bytes (not header-only).
- Error model is stable:
  - send to non-existent endpoint -> `IpcError::NoSuchEndpoint`
  - recv from empty queue -> `IpcError::QueueEmpty` (or `WouldBlock` for non-blocking API)
  - send to full queue -> `IpcError::QueueFull`
- Rights are enforced:
  - SEND without `Rights::SEND` -> `PermissionDenied`
  - RECV without `Rights::RECV` -> `PermissionDenied`

Suggested tests:

- Kernel unit tests for router semantics (already exists) + syscall-level tests for rights gating.

#### M2: capability bootstrap + transfer (required)

Acceptance:

- init spawns a service and transfers a bootstrap endpoint cap into a well-known slot.
- `cap_transfer` denies invalid rights masks; allows subset masks; never amplifies rights.

#### M3: policyd gating of authority (security milestone)

Acceptance:

- A service that is not authorized cannot obtain new endpoint caps / cannot communicate with a
  protected service (policy denial is observable and logged).

#### M4: de-stub VFS path (end-to-end)

Acceptance:

- `selftest-client` performs VFS `stat/open/read/close/ebadf` via **real IPC** (not mailbox) and
  emits success markers.

### Testing strategy (required for IPC/capability work)

This RFC is only useful if we can prevent “ABI drift” and “security regressions” while iterating.
Changes that touch IPC transport, capability enforcement, VMO/filebuffer flows, or policy gating
MUST come with tests at the appropriate layer(s) below.

#### 1) Unit tests (fast, always-on)

- Kernel:
  - router queue semantics (QueueFull/QueueEmpty)
  - rights gating (SEND/RECV/MAP) and “subset only” transfer rules
  - pointer/len validation helpers (overflow/bounds)
- Userland:
  - IDL encode/decode invariants (Cap’n Proto framing stays valid)
  - policy parsing/merging rules (deny reasons are deterministic)

#### 2) Property tests / fuzzing (security hot spots)

Focus areas:

- IPC syscalls: randomize pointers/lengths/flags/deadlines and assert:
  - no UB/panics
  - deterministic errno results
  - no out-of-bounds copy
- Capability operations: randomize rights masks, slot indices, and transfer sequences and assert:
  - no rights amplification
  - no slot leaks across failure paths
- IDL decode: malformed frames must fail safely (no allocator explosions, no infinite loops).

#### 3) Host integration tests (quick end-to-end without QEMU)

These should be the default “developer loop” for service-level semantics:

- Policy allow/deny flows (e.g. `tests/e2e_policy`)
- VFS and package flows (e.g. `tests/vfs_e2e`, `tests/e2e`)
- Remote/dsoftbus flows (e.g. `tests/remote_e2e`) to keep the future `softbusd` direction honest
  without requiring kernel networking today.

#### 4) QEMU E2E tests (gate boot + kernel ABI)

Keep the QEMU suite minimal but authoritative:

- Boot marker success (`RUN_UNTIL_MARKER=1`)
- One “IPC payload roundtrip” marker once v1 is implemented (proves copy-in/out correctness)
- Negative markers for rights violations (SEND/RECV without rights) to ensure the security model
  remains enforced on real hardware paths.

#### 5) ABI conformance checks (prevent silent breakage)

- Golden tests for:
  - `MsgHeader` size/layout (16 bytes, little-endian encoding)
  - syscall argument layouts (a0–a5) and errno mapping to `nexus_abi::IpcError`

These tests should fail loudly on any ABI change, forcing an explicit RFC update and review.

## Relationship to Other RFCs

- **RFC-0002**: Defines process-per-service; RFC-0005 defines the IPC/capability fabric between processes.
- **RFC-0003**: Logging must emit consistent errors for IPC denials/timeouts.
- **RFC-0004**: Loader and memory safety constraints apply to any shared-memory IPC design.

## Scalability & performance notes (universal OS)

This section is intentionally pragmatic: it highlights the design choices in this RFC that matter
for a future “universal OS” (phone/tablet/desktop/TV/auto/IoT) with high performance, strong
security, RISC‑V friendliness, and a future `softbusd` distributed layer.

### Control-plane vs data-plane (primary performance lever)

- **Control plane stays small**: endpoint IPC frames should remain “metadata + decisions” (Cap’n Proto).
- **Data plane uses VMO/filebuffer**: large bytes move out-of-band via VMO capabilities and explicit
  mapping, not megabytes of inline copies.
- **Why this scales**: phones/desktops want bandwidth; IoT wants predictable memory; automotive
  wants auditability. The “small message + explicit buffer” split is compatible with all three.

### Backpressure and queue sizing (latency vs throughput)

- Endpoint queues MUST have bounded depth (no unbounded alloc growth).
- Syscalls MUST provide non-blocking and deadline-based blocking semantics so services can pick:
  - low-latency (non-blocking + event loop)
  - high-throughput (blocking with bounded queues)
- “QueueFull/QueueEmpty” are not exceptional; they are normal flow-control signals.

### Copy-in/out now, zero/low-copy later (don’t overfit early)

- IPC v1 uses copy-in/out for small frames. This is fine for control-plane payload sizes.
- For “ultra fast”, we rely on the VMO/filebuffer data plane rather than prematurely building a
  complicated shared-memory message transport.
- If/when we introduce handle-attached messages (Fuchsia-like), it MUST preserve the same cap-slot
  authority model and must not reintroduce ambient authority.

Rationale (why not “handles attached to messages” as MVP):

- Attaching transferable handles to IPC messages is powerful, but it couples IPC transport, cap-table
  mutation, lifecycle, and backpressure into one kernel fastpath; for our current stage this is too
  easy to get subtly wrong and hard to test exhaustively.
- We already have a Rust-friendly hybrid (`cap_transfer` + IDL metadata + VMO/filebuffer) that keeps
  authority explicit and the ABI stable; once that is correct and fast, we can consider adding
  message-attached handles as an optimization, not as a prerequisite.

Criteria (when to consider message-attached handles later):

- We have **cap lifecycle primitives** (at minimum `cap_close` and clear ownership rules) and can
  prove no handle leaks across failure paths (timeout, queue full, task exit).
- We have **fuzz/property coverage** for IPC+caps interactions (including backpressure + timeouts).
- Profiling shows `cap_transfer` overhead or extra round-trips are a measurable bottleneck in real
  workloads (not synthetic microbenchmarks).
- The required semantics align with our OHOS-style service graph and future `softbusd` layering
  (i.e., no need to expose “remote handles” in the kernel ABI).

### Scheduler + blocking (avoid busy loops)

- The secure path MUST not require userland busy-yield loops to wait for IPC.
- Blocking syscalls should integrate with the scheduler (sleep/wakeup on endpoint readiness).
- This matters most on battery-bound devices (phones/tablets) and real-time constrained ones (auto).

### Policy cost containment (security without death-by-checks)

- The kernel enforces rights on held capabilities (fast, local checks).
- `policyd` handles “who should get which capability” decisions (potentially expensive).
- Bring-up milestone: `policyd` also gates **exec authorization** for `execd` (deny-by-policy returns
  a deterministic error and no child PID is spawned).
- For performance, policy decisions should be cacheable at userland boundaries (e.g., “cap request
  -> allow/deny”) without moving the policy engine into the kernel.

### RISC-V friendliness

- ABI uses a0–a5 and a single return register (a0): easy fastpaths, small stubs, predictable calling.
- Keeping bulk transfers as VMO mappings aligns with RISC‑V MMU strengths (ASIDs, page flags) and
  avoids copying large buffers through the trap path.

### `softbusd` layering (distributed without kernel ABI churn)

- `softbusd` MUST remain a userland service layered on the same local IPC fabric.
- Local services talk to `softbusd` via endpoint IPC; `softbusd` owns discovery, transport, crypto.
- This keeps the kernel ABI stable across device classes and avoids baking “network semantics” into
  local IPC syscalls.

## Appendix: Implementation checklist

This is a **dev-facing execution checklist** embedded here intentionally so RFC‑0005 remains a
single file.

### Kernel (neuron)

- **Syscall IDs pinned**
  - [x] `SYSCALL_SEND = 2` and `SYSCALL_RECV = 3` remain stable (no renumbering)
  - [x] `SYSCALL_CAP_TRANSFER = 8` remains stable (no renumbering)
  - [x] IPC v1/v2 syscalls are pinned (see `source/kernel/neuron/src/syscall/mod.rs`)

- **IPC transport v1**
  - [x] Implement `SYSCALL_IPC_SEND_V1 = 14` copy-in (header+payload)
  - [x] Implement `SYSCALL_IPC_RECV_V1 = 18` copy-out (header+payload)
  - [x] Backpressure behavior is observable (`QueueFull`, `QueueEmpty`, `NoSpace`)
  - [x] Non-blocking behavior is supported (no busy-loop requirements in userland)
  - [x] Blocking + deadlines are supported (kernel sleep/wakeup; no lost-wakeup)
  - [x] CAP_MOVE is supported (incl. blocking/timeout safety + rollback on recv NoSpace)

- **IPC transport v2 (recv metadata)**
  - [x] `SYSCALL_IPC_RECV_V2` returns `sender_service_id` (u64) via out-parameter (descriptor ABI)

- **Rights enforcement**
  - [x] SEND requires `Rights::SEND`
  - [x] RECV requires `Rights::RECV`
  - [x] Rights cannot be amplified by any syscall path

- **Capability transfer**
  - [x] `cap_transfer(child, parent_slot, rights_subset)` enforces subset masks
  - [x] Invalid rights mask fails deterministically
  - [x] `Rights::MANAGE` is non-transferable via `cap_transfer` (except `EndpointFactory` rule)

- **Fault containment**
  - [x] User page faults terminate + deschedule the offending task (no fault storms)
  - [x] Endpoint cleanup on task death prevents stranded waiters (close + waiter removal)

- **DoS / resource exhaustion hardening**
  - [x] Queue depth enforced (`KSELFTEST: ipc queue full ok`)
  - [x] Endpoint quotas enforced (`KSELFTEST: ipc endpoint quota ok`)
  - [x] Payload bytes accounting (endpoint/global/per-owner) enforced (`KSELFTEST: ipc bytes full ok`, `KSELFTEST: ipc global bytes budget ok`, `KSELFTEST: ipc owner bytes budget ok`)

### User ABI (nexus-abi)

- **Stable error mapping**
  - [x] `AbiError::from_raw` mapping is documented and kept stable for IPC-related errors
  - [x] IPC errno decoding covers `EAGAIN`/`ETIMEDOUT`/`ENOSPC` → `IpcError::{QueueFull/TimedOut/NoSpace}`

- **IPC wrappers (OS build)**
  - [x] Provide `ipc_send_v1`/`ipc_recv_v1` wrappers matching kernel transport v1 (no ad-hoc inline asm in apps)
  - [x] Provide `ipc_recv_v2` wrapper for recv-side metadata (`sender_service_id`)
  - [x] Provide `cap_clone`, CAP_MOVE flag, and endpoint factory/create syscalls used by init-lite/services

### IPC runtime (userspace/nexus-ipc)

- **Kernel backend**
  - [x] `KernelClient`/`KernelServer` for `nexus_env="os"` uses kernel syscalls (not a local mailbox)
  - [x] `Wait::{Blocking, NonBlocking, Timeout}` behavior maps cleanly onto kernel semantics

- **os-lite backend**
  - [ ] Remains available for bring-up, but clearly marked **non-security**

### Services (de-stub roadmap)

- **Bootstrap**
  - [x] init-lite transfers bootstrap endpoint caps to spawned services
  - [x] init-lite answers ROUTE_GET queries and returns per-service send/recv slots (bring-up routing responder)

- **policyd**
  - [ ] Denies unauthorized cap transfers / unauthorized service operations (authority still TODO)
  - [x] Emits stable markers for allow/deny cases used by selftests
  - [x] Rejects malformed frames deterministically (selftest marker)
  - [x] Rejects spoofed requester identity by binding to sender_service_id (except init-lite proxy)

- **samgrd**
  - [x] Implements minimal registry proto v1 (REGISTER/LOOKUP of per-client slot tuples) over real IPC
  - [x] Rejects malformed frames deterministically (selftest marker)

- **bundlemgrd**
  - [x] Implements minimal bundle proto v1 (LIST, returns 0 for now) over real IPC
  - [x] Rejects malformed frames deterministically (selftest marker)

- **keystored**
  - [x] Implements minimal keystore proto v1 (PUT/GET/DEL) over real IPC
  - [x] Rejects malformed frames deterministically (selftest marker)

- **execd**
  - [x] Implements minimal exec proto v1 (exec image selector) over real IPC (bring-up)
  - [x] Rejects malformed frames deterministically (selftest marker)
  - [x] Binds requester identity to IPC sender channel (rejects spoofed `requester` field)

- **vfsd**
  - [x] Implements `stat/open/read/close` over real IPC (see `SELFTEST: vfs * ok` / `SELFTEST: vfs real data ok`)

### Tests (acceptance)

- **Kernel**
  - [x] Syscall-level tests: SEND/RECV rights denial, QueueEmpty/QueueFull behavior
  - [x] cap_transfer tests: subset masks and invalid mask rejection

- **QEMU E2E**
  - [x] `RUN_UNTIL_MARKER=1` passes with VFS checks running over real IPC
  - [x] policyd allow/deny is exercised and visible via UART markers
  - [x] policyd requester spoof denial is exercised (`SELFTEST: policyd requester spoof denied ok`)
  - [x] policyd malformed-frame negative case is exercised and visible via UART marker
  - [x] keystored proto v1 is exercised and visible via UART marker
  - [x] execd proto v1 is exercised and visible via UART marker
  - [x] policyd-gated exec denial is exercised (`SELFTEST: exec denied ok`)
  - [x] Sender identity binding is exercised (`SELFTEST: ipc sender service_id ok`)

Notes:

- QEMU marker suite includes `SELFTEST: ipc payload roundtrip ok` and
  `SELFTEST: ipc deadline timeout ok` as minimal proof that IPC v1 payload copy
  and deadline semantics are working end-to-end, plus routing + VFS checks over
  real cross-process kernel IPC.
