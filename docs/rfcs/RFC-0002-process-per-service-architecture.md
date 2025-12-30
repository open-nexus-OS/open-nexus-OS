# RFC-0002: Process-Per-Service Architecture

- Status: Complete (Phase 0 complete; later phases ongoing/deferred)
- Owners: Runtime + Kernel Team
- Created: 2025-10-24
- Last Updated: 2025-12-18

## Status at a Glance

- **Phase 0 (Process-per-service + kernel exec as sole loader path)**: Complete ✅
- **Phase 1 (Service maturity / full orchestration)**: In progress / deferred (tracked in RFC‑0005 and service proto work)

Definition:

- In this RFC, “Complete” means the architecture decision is fully implemented: services are separate
  ELFs, kernel `exec`/`exec_v2` is the sole loader path, and `init-lite` is a minimal wrapper/packager.

## Scope boundaries (anti-drift) + TASK-0002 alignment

This RFC intentionally stays narrow: it defines **process-per-service** and the **kernel `exec` loader path**.
To avoid “spec drift” and duplicated authority across documents:

- **RFC-0002 owns**:
  - Process-per-service split (each service is a separate ELF/process).
  - Kernel `exec`/`exec_v2` as the sole loader path (init does not map ELFs itself).
  - The role of `init-lite` as a thin wrapper/packager and orchestrator of spawning.
- **RFC-0002 explicitly does NOT own**:
  - IPC syscall/ABI details, endpoint semantics, capability rights/transfer rules, or bootstrap endpoint patterns → **RFC‑0005 owns this**.
  - Logging control plane / sinks / routing → **RFC‑0003**.
  - Loader/mapping safety invariants (W^X, zero-init, provenance, guard VMAs) → **RFC‑0004**.
  - Service protocol details (e.g. VFS opcodes, policy wire formats) → owned by the service modules + their ABI docs/RFC‑0005 where applicable.

### Relationship to tasks (execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions + proof**.
- This RFC defines the architecture/contract; tasks implement and prove it.

### TASK-0002 (Userspace VFS Proof): what it is / isn’t

TASK‑0002 is an **implementation + proof checklist**. If it ever contradicts this RFC, **this RFC wins**.

- **TASK‑0002 includes (as far as RFC‑0002 is concerned)**:
  - Prove `init-lite` spawns real service processes through kernel `exec`.
  - Prove userspace VFS behavior via real cross-process IPC (with RFC‑0005 providing the IPC fabric).
- **TASK‑0002 explicitly does NOT include** (tracked by separate tasks/RFCs):
  - SMP bring-up / SBI HSM/IPI / per-hart timers (e.g. `TASK-0247`).
  - Virtio-blk device access model / userspace drivers (e.g. `TASK-0010`, `TASK-0246`, `TASK-0247`).
  - Persistence/statefs, `/state` mounting semantics (e.g. `TASK-0009`).
  - OOM watchdog / memstat (e.g. `TASK-0228`).
  - Driver/accelerator tracks (e.g. `TRACK-DRIVERS-ACCELERATORS.md`).
  - Distributed IPC/softbus layering (separate distributed docs).

## Context

Current architecture attempts to link all services (keystored, policyd, samgrd, bundlemgrd, packagefsd, vfsd, execd) into a single no_std binary (nexus-init with os-lite feature). This creates insurmountable dependency conflicts:

1. **Cargo limitation**: `target.'cfg(...)'` conditional dependencies don't prevent compilation, only linking
2. **Std leakage**: Services transitively pull std dependencies (serde, thiserror, parking_lot, capnp, der)
3. **Maintenance burden**: Every service dependency must be audited for no_std compatibility
4. **Security**: Single-address-space services violate process isolation principles

The TASK-0002 blocker: Cannot build nexus-init or selftest-client for `riscv64imac-unknown-none-elf` target because service dependencies require std.

## Decision

Adopt **process-per-service architecture** where each service is an independent ELF binary:

### Architecture

```text
Kernel
  └─> spawns init (minimal, ~100 lines, only nexus-abi dependency)
       └─> init loads and spawns service ELFs from embedded storage:
            ├─> keystored.elf
            ├─> policyd.elf  
            ├─> samgrd.elf
            ├─> bundlemgrd.elf
            ├─> packagefsd.elf
            ├─> vfsd.elf
            └─> execd.elf
```

### Components (revised)

1. **Minimal init (thin wrapper)**  
   - `source/apps/init-lite` is now only a wrapper: it packages embedded service ELFs and immediately delegates to `nexus-init`, which in turn invokes the kernel `exec` path.  
   - No userspace ELF parsing/mapping remains; init forwards bytes + stack/args and emits only the UART markers.

2. **Kernel exec loader (primary path)**  
   - Kernel-side `exec` parses ELF, allocates VMOs, maps PT_LOAD with W^X plus guard VMAs, provisions stack/regs, and spawns the task.  
   - Capability-gated so only init can invoke it.  
   - Per-service isolation, stack/W^X, and guard-VMAs are fulfilled in the kernel (RFC‑0004 alignment).

3. **Service binaries** (each service's `src/main.rs`)  
   - Build independently for `riscv64imac-unknown-none-elf`.  
   - Use conditional `#[cfg(nexus_env = "os")]` for no_std/no_main; host keeps std for tests.  
   - Process-per-service remains unchanged.

4. **Embedded storage**  
   - Initial: Include service ELFs in kernel image via `include_bytes!`.  
   - Future: Ramdisk filesystem or early storage driver.  
   - Init forwards the bytes to the kernel exec loader; no local mapping in userspace.

### Staging focus (phase gating)

- **Phase 0a (must-pass now)**: Achieve first userspace span without faults (init-lite → kernel exec → first service runs). Defer the full service stack orchestration (samgrd, bundlemgrd, packagefsd/vfsd/execd policies) until this is green.
- **Phase 1+ (after first span)**: Bring up registry/filesystem/lifecycle orchestration once a clean user-mode path exists; wire policies and sequencing then.

### Migration Path

#### Phase 1: Infrastructure (blocking TASK-0002)

- [x] kernel: Add `embed_init` cfg and EMBED_INIT_ELF build variable
- [x] kernel/selftest: Implement minimal ELF loader using existing spawn infrastructure
- [x] Verify init-lite can be embedded and spawned as separate process
- [x] Emit basic userspace markers to prove kernel→userspace works

#### Phase 2: Service Binaries

- [x] Service entry wrappers per crate (OS: `nexus_service_entry::declare_entry!`; stubs explicitly marked):
  - [x] `keystored`: os-lite stub emits readiness marker
  - [x] `policyd`: os-lite stub emits readiness marker
  - [x] `samgrd`: os-lite stub emits readiness marker
  - [x] `bundlemgrd`: os-lite stub emits readiness marker
  - [x] `execd`: os-lite stub emits readiness marker
  - [x] `debugsvc`: already uses `nexus_service_entry::declare_entry!` under `no_std`
  - [x] `packagefsd`: wraps `service_main_loop` via `declare_entry!`
  - [x] `vfsd`: wraps `service_main_loop` via `declare_entry!`
- [x] Update each service's `Cargo.toml` to provide OS bins and an `os-lite` feature.
- [x] Build service ELFs via `scripts/run-qemu-rv64.sh` (`--no-default-features --features os-lite`).
- [ ] Init packaging + guard hygiene (ties into RFC‑0003/0004):
  - [x] Relocate the generated `ServiceImage` tables (names + include_bytes! blobs) into `.rodata` so they never overlap the `.bss` guard fence referenced by RFC‑0003/0004.
  - [x] Move the `__small_data_guard` symbol into a dedicated `NOLOAD` section to keep the guard address range distinct from real data and stop `nexus_log` from faulting when probing service metadata.
  - [~] Map text/data segments with disjoint VMOs and enforce W^X at the kernel `as_map` boundary (see RFC‑0004 status update). Kernel-side W^X + the dedicated VMO arena are in place; explicit `PROT_NONE` guards and scratch-page zeroing remain.

#### Phase 3: Init Integration (kernel exec)

- [x] Add privileged kernel `exec` syscall:
  - Parse ELF PT_LOAD segments, allocate VMOs, map with W^X + guard VMAs.
  - Provision stack/regs, set entry/gp, spawn the task.
  - Gate by capability so only init can invoke it.
- [x] Rework init-lite/nexus-init:
  - Enumerate packaged ELFs and call `exec` per service (stack pages, args, gp).
  - Transfer caps per service after spawn (bootstrap slot policy unchanged).
  - Emit readiness markers; continue on non-critical failures.

#### Phase 4: Simplification & Testing

- [x] Remove the redundant `os_lite` bootstrap path; keep only thin init-lite → kernel-exec.
- [x] Update `scripts/run-qemu-rv64.sh` to stage ELFs and use kernel exec.
- [x] Smoke: `RUN_UNTIL_MARKER=1 just test-os` verifies `init:*` + service `*: ready` markers and exits 0.
- [ ] VFS workflow: **deferred** until RFC‑0005 kernel IPC v1 (process-per-service breaks os-lite in-process mailbox IPC by design).

## Rationale

**Benefits:**

- **Eliminates dependency hell**: Each service builds independently with its own dependency closure
- **Process isolation**: Services in separate address spaces (security, fault isolation)
- **Incremental migration**: Can convert services one at a time
- **Matches ADR-0001**: Clear role boundaries (init, loader, services)
- **Production-ready**: Standard OS architecture

**Costs:**

- **More binaries**: Must build and embed multiple ELFs (init + N services)
- **Loader complexity**: Init needs ELF loading capability (already exists in nexus-loader)
- **Boot time**: Sequential service loading/spawning (acceptable for embedded)

**Alternatives considered:**

1. ❌ **Fix service std dependencies**: Massive audit/refactor, ongoing maintenance burden
2. ❌ **Cargo features per dependency**: Doesn't solve transitive dependencies, complex
3. ❌ **Single-address-space services**: Current approach, fundamentally broken
4. ✅ **Process-per-service**: Industry standard, clean architecture

## Consequences

**Immediate (TASK-0002):**

- Can complete userspace VFS proof by spawning real service processes
- Markers come from actual running services, not fakes
- Proves end-to-end userspace IPC and VFS functionality

**Long-term:**

- Services can use any dependencies (std or no_std) as appropriate
- Better security and fault isolation
- Easier to test services independently
- Aligns with production OS architecture

## Open Questions

1. **Storage format**: Embed ELFs as separate blobs or in tar/cpio archive?
   - **Recommendation**: Start with individual `include_bytes!`, migrate to ramdisk format later

2. **Capability distribution / IPC semantics**: How does init know which caps to give each service, and what are the stable IPC/capability rules?
   - **Recommendation**: This is defined in RFC-0005 (Kernel IPC & Capability Model). RFC-0002 only defines the process-per-service split.

3. **Service dependencies**: How to handle servicevice startup ordering (e.g., vfsd needs packagefsd)?
   - **Recommendation**: Simple sequential launch order in init, no dependency graph yet

4. **Failure handling**: What if a service fails to load/start?
   - **Recommendation**: Log error and continue (don't block other services), revisit with supervision

## Checklist (complete)

- [x] Process-per-service is the architecture decision (services are separate processes).
- [x] Kernel `exec`/`exec_v2` is the sole loader path; init does not map ELFs itself.
- [x] This RFC stays scoped; IPC/capability contracts are owned by RFC‑0005.
- [x] TASK‑0002 proof path is compatible with this RFC (no fake success, deterministic markers).

## References

- ADR-0001: Runtime Roles & Boundaries
- ADR-0002: Nexus Loader Architecture
- ADR-0017: Service Architecture
- TASK-0002: Userspace VFS Proof (`tasks/TASK-0002-userspace-vfs-proof.md`)
- RFC-0005: Kernel IPC & Capability Model
