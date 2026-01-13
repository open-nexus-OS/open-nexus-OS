# Service architecture (onboarding)

This page explains how “services” are structured in Open Nexus OS and how that connects to the host-first workflow.

Canonical decision record:

- `docs/adr/0017-service-architecture.md` (service architecture direction)

## What is a “service” here?

A service is a **process** (daemon) under `source/services/<name>d` that:

- registers with the service manager (`samgrd`),
- exposes Cap’n Proto IDL interfaces,
- validates inputs and propagates rich errors,
- and forwards into a corresponding **userspace domain library** compiled for `nexus_env="os"`.

The goal is that business rules remain host-testable and safe.

## Host-first structure (domain vs adapter)

- **Domain libraries** live in `userspace/` and are designed to run on the host:
  - unit/property tests
  - contract tests
  - Miri (where applicable)
- **Daemons** in `source/services/*d` are thin adapters:
  - wiring and lifecycle
  - IPC/IDL translation
  - deterministic readiness markers

If service code starts accumulating “real logic”, that’s usually a sign the boundary is leaking.

## How services communicate

### Hybrid pattern: Control Plane + Data Plane

All services follow a **hybrid architecture** that separates control (small, structured) from data (large, bulk):

#### Control Plane (IPC/RPC)

**Purpose**: Small, structured requests/responses (registration, queries, commands)

| Backend | Usage | Wire Format |
|---------|-------|-------------|
| **Host/std** | Type-safe tests, evolvable APIs | Cap'n Proto (IDL schemas in `tools/nexus-idl/schemas/`) |
| **OS/os-lite** | Minimal QEMU bring-up | Versioned byte frames (magic + version + ops) |

**Constraints**:
- Keep messages small (typically <1KB, max ~4KB for IPC frame budget)
- All sizes bounded and validated before allocation
- IDL schema serves as documentation even when byte frames are authoritative (OS)

**Examples**:
- `samgrd`: Register/Resolve service endpoints
- `bundlemgrd`: Install/Query bundle metadata
- `logd`: Append/Query/Stats log records
- `keystored`: Sign/Verify operations (signature only, not key material)

#### Data Plane (Bulk Payloads)

**Purpose**: Large payloads (>4KB) that should not be copied inline

| Mechanism | Usage | When |
|-----------|-------|------|
| **VMO (Virtual Memory Object)** | Zero-copy bulk sharing (kernel-backed) | OS production (TASK-0031+) |
| **Filebuffer** | Host testing, exports, debugging sinks | Host tests, optional exports |

**Pattern** (RFC-0005 "Bulk buffer pattern"):
1. Producer allocates VMO and writes bytes
2. Producer sends metadata inline (VMO handle + size/offset) via Control Plane IPC
3. Producer transfers VMO capability to consumer
4. Consumer maps VMO and consumes bytes
5. Consumer closes capability when done

**Examples**:
- `bundlemgrd`: Bundle artifact bytes (install payload via VMO)
- `vfsd`/`packagefsd`: File content reads (future: map via VMO)
- `logd` (v2+): Bulk log scrape for remote observability (TASK-0040)
- `dsoftbus`: Large payloads over network (chunked, future: VMO-backed)

**v1 Allowance**:
- Small bulk payloads (<4KB) MAY be sent inline if bounded and convenient
- UART mirror, file exports, debug sinks are **outputs**, not backend patterns

**Consistency rule**:
- Control Plane: Always hybrid (Cap'n Proto + byte frames)
- Data Plane: Inline (if small) or VMO/filebuffer (if large)

### Transport and capability semantics

Transport layer and capability semantics are kernel-defined:

- **Schemas**: `tools/nexus-idl/` (`*.capnp`)
- **Generated runtime**: `userspace/nexus-idl-runtime`
- **IPC model**: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- **Hybrid contract rationale**: `docs/distributed/dsoftbus-lite.md` (Control + Data split)

## Readiness + proof (no fake green)

Services must not claim readiness unless they truly are ready.

Typical marker responsibility split:

- init orchestrator prints `init: start <svc>` / `init: up <svc>`
- each service prints `<svc>: ready` once it can accept requests
- `scripts/qemu-test.sh` enforces marker ordering/presence

## Where to add tests

- **Most behavior**: add tests in the userspace crate.
- **Integration flows**: add host E2E tests under `tests/` (fast, deterministic).
- **Bare-metal smoke**: rely on `scripts/qemu-test.sh` and keep proofs bounded.

## Useful entry points

- Testing methodology: `docs/testing/index.md`
- Tasks workflow: `tasks/README.md`
- Layering and quick reference: `docs/ARCHITECTURE.md`
