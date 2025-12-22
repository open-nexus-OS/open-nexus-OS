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

```
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

## Implementation Status

### Completion snapshot (2025-12-12)
- Overall: ~75% complete. Kernel `exec` path and init-lite wrapper migration are done; services still need full no_std ports and final os_payload removal once exec is the sole loader.

### Phase 1: Kernel ELF Loader ✅ COMPLETE
- Kernel: ELF embedding infrastructure (build.rs, check-cfg)
- Kernel: ELF64 parser in selftest
- Kernel: User page allocation and mapping
- Kernel: Spawn with separate AS and U-mode (SPP=0)
- Init spawns correctly (PID 5, entry=0x10400000, sp=0x20000000)

### Phase 2: Userspace Scheduling ✅ COMPLETE
- Init-lite built, embedded, and scheduled
- Dynamic trap runtime replaces static `TRAP_ENV` (per-task trap domains)
- Context switch path uses pure Rust + inline asm (no `#[naked]`, no post-satp cleanup)
- U-mode syscalls execute repeatedly; init-lite prints `init: start` marker via `SYSCALL_DEBUG_PUTC`
- User page fault at `0x10000000` fixed by relocating init-lite ROM to `0x10400000`

- ### Phase 3: Service Binaries (Audit 2025-11-24)
- Host-oriented wrappers for `policyd`, `samgrd`, `bundlemgrd`, and `execd` still depend on `std` transport stacks and only expose `fn main()`.
- `debugsvc`, `packagefsd`, and `vfsd` integrate `nexus_service_entry::declare_entry!` with `no_std` gating and compile for the OS target.
- `keystored` now offers an os-lite stub (`nexus_service_entry` + cooperative loop) while the full Cap'n Proto transport remains host-only.
- Primary blockers: keystore/policy crates require filesystem and heap abstractions from `std`; need feature gates or replacements prior to no_std builds.
- Next steps: refactor remaining services onto `nexus_service_entry`, gate `std` usage, and add OS `[[bin]]` manifests.

### Phase 3: Service Binaries (Next)
- [ ] Service binaries: migrate remaining crates to `nexus_service_entry` and `no_std`
- [x] Init: ELF loading and multi-service spawn (env-driven bundle; awaiting full service roster)
- [x] Relocate loader/logging logic into `nexus-init` so the deprecated `init-lite` binary is only a payload wrapper
- [ ] Scripts: Build and embed all service ELFs (helper plumbing landed, needs real no_std builds)
- [ ] Testing: End-to-end with real services

### Phase 4: Testing (Next)
- [x] Update `scripts/run-qemu-rv64.sh` to embed init + service ELFs
- [ ] Verify marker sequence with real service processes
- [ ] Run VFS operations from selftest-client against real vfsd/packagefsd

### Known Gaps
- VFS E2E via cross-process IPC is deferred until RFC‑0005 kernel IPC v1.

### Recent Progress (2025-11-18)
- Address-space switch now atomic: `activate()` + context restore fused into `context_switch_with_activate`
- Trap handler hardened with per-task trap domains; resolves `sepc` corruption/double-fault
- Logging path moved to `nexus-log`; temporary uart probes guard against unsafe pointers until Phase 0a lands (RFC-0003)
- User entry migrated to `nexus_service_entry::declare_entry!`; no kernel `#[naked]` functions
- `init-lite` relinked to `0x10400000`; first user trap prints correct 16-digit diagnostics
- QEMU runs show sustained ECALLs without kernel faults; groundwork ready for spawning additional services
- Init build script now consumes `INIT_LITE_SERVICE_LIST` + per-service ELF env vars, parses ELF headers directly, and maps segments without `nexus-loader`
- Current blocker: logging guard detects runaway string lengths (e.g. `len=0x3b0fff10000`), pointing to pending Phase 0a hardening (RFC-0003 / RFC-0004). Temporary probes stay enabled while we rework the StrRef/VMO pipeline.

### Recent Progress (2025-11-24)
- Audited service binaries to document which crates already expose `_start` via `nexus_service_entry` (`debugsvc`, `packagefsd`, `vfsd`) and which remain host-only (`keystored`, `policyd`, `samgrd`, `bundlemgrd`, `execd`).
- Captured outstanding blockers around `std`-only dependencies (filesystem-backed keystore, policy registries) ahead of the no_std porting work.
- Landed an os-lite keystored stub that emits readiness through `nexus_service_entry`, establishing the pattern for future service shims.

### Historical Debug Notes (2025-11-02)
- Initial trap handler crash traced to stale static pointers after AS switch
- Dynamic trap runtime and safe logging introduced to eliminate the failure mode

## Execution Plan (Top-Down)

### 0. Exec Syscall Shape (kernel)
- Add privileged `exec` (or loader-handle) syscall, gated by an init-only capability:
  - Input: ELF bytes (ptr+len), stack_pages, gp, args/env (future), optional entry override.
  - Kernel: parse PT_LOAD, allocate VMOs, map with W^X + guard VMAs, zero BSS, set stack/regs, spawn task in a fresh AS.
  - Output: pid/handle; errors are ABI codes (bad ELF, prot violation, cap denied).

### 1. Capability Contracts & Launch Policy
- Produce a per-service capability matrix covering bootstrap endpoint, IPC rights, required VMOs and address-space handles.
- Document expected startup order (`init → execd → packagefsd → vfsd → bundlemgrd → policyd → keystored → samgrd`) with readiness markers.
- Derive initial policy for capability transfers in init-lite (source slots, rights masks, target slots).
- Capture fallbacks for optional services (e.g., absence of policyd should not block vfsd bring-up).
- Add the matrix and policy narrative to `docs/rfcs/RFC-0002` and reference ADR-0017 once finalized.

#### Bootstrap Cap Matrix (initial scope)
- **identityd**
  - Needs: identity/auth capability handle (read), IPC bootstrap endpoint, read access to keystore public data, no write to keystore storage.
  - Provided by init: identity cap in target slot, IPC default target set, no VMO with secrets.
- **keystored**
  - Needs: keystore storage VMO/cap (read/write), IPC bootstrap endpoint, optional RNG/crypto service cap.
  - Provided by init: storage cap in target slot with RW, IPC default target set, rng cap if available.
- **Common**
  - Each service gets its own AS/stack via `exec`; bootstrap cap stays in slot 0 per policy; additional caps are rights-filtered before transfer.

### 2. Service Binary Scaffolding
- Introduce a shared `service-entry` crate that provides `_start`, panic handler, and syscall shims under `nexus_env = "os"`.
- Update each service crate (`keystored`, `policyd`, `samgrd`, `bundlemgrd`, `packagefsd`, `vfsd`, `execd`) with:
  - `[[bin]]` target for OS builds.
  - Feature gates to select between host/std main and OS `_start`.
  - Integration tests that validate the thin entry layer on the host using `cargo test --features std`.
- Ensure services compile standalone for `riscv64imac-unknown-none-elf` with only `nexus-abi` and minimal dependencies enabled.

### 3. Init-Lite Orchestration (kernel exec)
- Short-term packaging: keep `include_bytes!` per service (archive later).
- Init-lite enumerates packaged ELFs and calls kernel `exec` (or loader-handle) per service, then transfers caps per policy.
- Emit `service:<name>:spawned/ready` markers; continue on non-critical failures, panic on critical ones.
- Build-time env stays (`INIT_LITE_SERVICE_LIST`, `INIT_LITE_SERVICE_<NAME>_ELF/STACK_PAGES`); scripts stage defaults.

### 4. Tooling & Validation
- Update `scripts/run-qemu-rv64.sh` to embed the full service bundle and capture the readiness markers.
- Add a smoke test that boots QEMU, waits for `init: ready` plus all service markers, and asserts no kernel traps were emitted.
- Expand selftest-client to exercise a minimal cross-service workflow (e.g., request file listing via `vfsd` backed by `packagefsd`).
- Track outstanding work with GitHub issues per service, linking back to this RFC section for scope alignment.

## References

- ADR-0001: Runtime Roles & Boundaries
- ADR-0002: Nexus Loader Architecture  
- ADR-0017: Service Architecture
- TASK-0002: Userspace VFS Proof
- `userspace/nexus-loader/`: Existing ELF loader implementation
