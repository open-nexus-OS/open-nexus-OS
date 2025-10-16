# Service lifecycle notes

## Service startup order (scaffold)

The init process currently brings up services in the following sequence while
the platform is under construction:

- `init`
- `keystored`
- `policyd`
- `samgrd`
- `bundlemgrd`
- `init: ready`

Each stub emits a `*: ready` marker on the UART. `nexus-init` prints matching
`*: up` confirmations as it observes readiness so the QEMU harness can enforce
the order deterministically.

The kernel banner marker to expect in logs is `neuron vers.` rather than `NEURON`.

## Execution pipeline

Service launch now flows through the policy gate:

1. `bundlemgrd` exposes each installed bundle's capability requirements via
   `QueryResponse.requiredCaps`.
2. `nexus-init` asks `policyd.Check` whether the service may consume those
   capabilities. Denials are logged as `init: deny <name>` and the service is
   skipped.
3. When `policyd` approves, init forwards the request to `execd` which performs
   the actual launch (stubbed today to emit `execd: exec <name>`).

This pipeline applies to every non-core service defined under `recipes/services/`.

### Planned bundlemgrd → execd integration (v1.1)

The initial loader work in v1 keeps `execd` focused on embedded payloads so we
can validate the address-space plumbing without touching service IDL. The next
increment extends `bundlemgrd` with an RPC that returns a read-only VMO handle
for the requested bundle's ELF image (or, equivalently, provides the bytes via
a shared buffer). `execd` will call the new method, receive the VMO, and hand it
to the loader instead of relying on statically linked test assets. Capability
policy and install/query semantics remain unchanged; only the data plane between
`bundlemgrd` and `execd` grows a handoff for ELF bytes.

## Loader v1 (PT_LOAD only)

The first iteration of the userland loader is intentionally narrow: it accepts
ELF64 binaries targeting RISC-V, processes PT_LOAD segments only, and enforces
W^X at plan time. The flow is:

1. `execd::exec_elf_bytes` invokes `nexus_loader::parse_elf64_riscv` to build a
   `LoadPlan`. Segment metadata is validated (magic/class/machine, page-aligned
   virtual addresses, `filesz <= memsz`, and no RWX mappings).
2. `as_create` provisions a fresh Sv39 address space. A staging VMO is carved
   out via `vmo_create`, filled with the ELF image, and wrapped in
   `OsMapper`—`load_with` walks the PT_LOAD headers in ascending `p_vaddr`,
   translating protections into kernel flags before calling `as_map`.
3. `StackBuilder` allocates a private stack (RW pages + guard), copies the
   string table for `argv`/`env` into the backing VMO, and returns both a
   16-byte aligned stack pointer and the child virtual addresses for the tables.
4. `spawn` receives the entry PC, stack SP, address-space handle, and bootstrap
   endpoint slot. A placeholder `BootstrapMsg` records `argc`, the table
   pointers, and the seed endpoint; future revisions will plumb these fields via
   IPC once the kernel exposes the hook.

The loader defers policy decisions (e.g. bundle provenance) to higher layers
and intentionally relies on embedded payloads for v1. The kernel still enforces
W^X when installing mappings; the userspace planner acts as the first filter so
misbehaving binaries are rejected before any syscalls are issued.

## Minimal exec path (MVP)

`execd` now exposes `exec_minimal(subject)` as a bootstrap-friendly path while
the real ELF/NXB loaders are being implemented. The handler:

1. carves a temporary, shared stack from a static buffer (the kernel still runs
   all tasks in a single address space at this stage),
2. reuses slot `0` of its capability table as the bootstrap endpoint, seeding
   the child with send-only rights via `cap_transfer`,
3. issues the `spawn` syscall targeting the `hello_child_entry` payload from
   `userspace/exec-payloads`, and
4. logs `execd: spawn ok <pid>` once the kernel acknowledges the request.

The child prints `child: hello` and yields forever, proving that the task was
scheduled. `selftest-client` emits `SELFTEST: e2e exec ok` after invoking the
new handler so the QEMU harness can assert the full sequence. The shared stack
and address space are strictly temporary; per-task address spaces land in the
next milestone.
