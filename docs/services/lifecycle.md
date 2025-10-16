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

### Loader v1.1 (service path)

The loader is now wired through the service pipeline. Once a bundle has been
installed `execd::exec_elf` asks `bundlemgrd` for the payload bytes via the new
`getPayload` opcode. The daemon looks up the bundle, resolves the staged
`payload.elf`, and returns it to the caller. `execd` immediately creates a fresh
address space with `as_create`, stages the ELF into a VMO, and feeds it to the
loader:

1. `bundlemgrd.getPayload` validates that the bundle is installed and streams
   the stored `payload.elf` bytes back over IPC.
2. `execd.exec_elf` writes the bytes into a staging VMO and calls
   `nexus_loader::load_with`, which enforces ELF64/EM_RISCV/PT_LOAD constraints,
   rejects W^X mappings, and orders segments strictly by virtual address.
3. `OsMapper` translates loader protections into kernel flags, zero-fills the
   `.bss` tail, and invokes `as_map` for each PT_LOAD segment.
4. `StackBuilder` provisions a private stack (guard page + RW pages), builds the
   argv/env string tables, and hands the stack pointer plus table addresses back
   to the caller.
5. `spawn` receives the entry PC, stack SP, and address-space handle. The child
   prints `child: hello-elf` and yields, proving that the mapped payload ran.

Both the loader and the Sv39 mapper deny write+execute pages. Misaligned
segments, truncated data, and overflows are rejected before any syscalls land,
so `execd` reports clear failures when a bundle is malformed.

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

`execd` still exposes `exec_minimal(subject)` as a bootstrap-friendly path while
the loader matures. The handler:

1. carves a temporary, shared stack from a static buffer (the kernel still runs
   all tasks in a single address space at this stage),
2. reuses slot `0` of its capability table as the bootstrap endpoint, seeding
   the child with send-only rights via `cap_transfer`,
3. issues the `spawn` syscall targeting the `hello_child_entry` payload from
   `userspace/exec-payloads`, and
4. logs `execd: spawn ok <pid>` once the kernel acknowledges the request.

The child prints `child: hello-elf` and yields forever, proving that the task was
scheduled. `selftest-client` emits `SELFTEST: e2e exec ok` after invoking the
handler so the QEMU harness can assert the full sequence. The shared stack and
address space are strictly temporary; per-task address spaces land in the next
milestone.
