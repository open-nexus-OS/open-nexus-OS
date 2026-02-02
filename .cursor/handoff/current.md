# Handoff (Current)

<!--
CONTEXT
This is the entry-point for a new chat/session.
Keep it short, factual, and proof-oriented.
Update it at the end of each task.
-->

## What was just completed
- **task**: `tasks/TASK-0010-device-mmio-access-model.md` (Status: Done)
- **contracts**:
  - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (Status: Done)
  - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (capability distribution model)
  - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy enforcement)
- **touched_paths**:
  - `source/kernel/neuron/` (syscalls: DEVICE_CAP_CREATE, CAP_TRANSFER_TO, negative tests)
  - `source/libs/nexus-abi/` (userspace wrappers)
  - `source/init/nexus-init/` (policy-gated MMIO distribution, dynamic virtio probing)
  - `source/services/{rngd,netstackd,virtioblkd}/` (MMIO consumers)
  - `source/services/policyd/` (privileged proxy for init)
  - `recipes/policy/base.toml` (device.mmio.{net,rng,blk} capabilities)
  - `scripts/{qemu-test.sh,run-qemu-rv64.sh}` (virtio-blk device, markers, RUN_PHASE=mmio)
  - `justfile` (test-mmio target)
  - `docs/rfcs/`, `docs/testing/`, `docs/architecture/` (RFC-0017, device-mmio-access.md, contracts-map.md)

## Proof (links / commands)
- **tests**:
  - `just fmt-check` ✅
  - `just lint` ✅
  - `just deny-check` ✅
  - `just test-host` ✅
  - `just test-e2e` ✅
  - `just arch-check` ✅
  - `just dep-gate` ✅ (critical: no forbidden crates)
  - `just build-kernel` ✅
  - `just test-os` ✅ (all markers including new policy deny test)
  - `just test-mmio` ✅ (fast local MMIO phase test)
  - `make test` ✅ (261 tests via nextest)
  - `make build` ✅ (host + OS services + kernel)
  - `make run` ✅ (QEMU boot + all markers)
- **qemu markers (all green)**:
  - `SELFTEST: mmio map ok` ✅
  - `rngd: mmio window mapped ok` ✅
  - `virtioblkd: mmio window mapped ok` ✅ (NEW - virtio-blk consumer proof)
  - `SELFTEST: mmio policy deny ok` ✅ (NEW - policy deny-by-default proof)

## Current state summary (compressed)
- **why**:
  - Userspace drivers can now safely access device MMIO windows via capability-gated syscalls
  - Init distributes device capabilities dynamically (policy-gated, audited, per-device windows)
  - Kernel enforces W^X at MMIO boundary (USER|RW only, never EXEC)
  - virtio-blk MMIO access proven end-to-end (unblocks TASK-0009 persistence)
- **new invariants / constraints**:
  - Device MMIO mappings MUST be capability-gated (no ambient access)
  - MMIO caps MUST be distributed by init (policy-checked, sender_service_id bound)
  - MMIO mappings MUST be non-executable (W^X enforced at page table level)
  - Per-device windows MUST be bounded to exact device BAR (least privilege)
  - Policy decisions MUST be audited via logd (no secrets in logs)
- **known risks**:
  - DMA/IRQ delivery not yet implemented (follow-up tasks required)
  - virtio virtqueue operations beyond basic MMIO probing (follow-up)

## Next steps (drift-free)
1. **TASK-0009: Persistence v1 (virtio-blk + statefs)** - READY TO START
   - **First action**: Create RFC seed for statefs journal format (use `docs/rfcs/RFC-TEMPLATE.md`, update `docs/rfcs/README.md`)
   - virtio-blk MMIO access is proven (TASK-0010 Done)
   - Implement statefs journal engine (host-first: BlockDevice trait + mem backend)
   - Create `statefsd` service (journaled KV store over block device)
   - Migrate keystored device keys to `/state/keystore/*`
   - Migrate updated bootctl to `/state/boot/bootctl.*`
   - Prove persistence via soft reboot (restart statefsd, replay journal, verify data)
   - QEMU markers: `blk: virtio-blk up`, `statefsd: ready`, `SELFTEST: statefs persist ok`
   - **Progressive**: Update RFC checkboxes as phases complete
   - **Final**: Update RFC status to Complete when all proofs green

## Blockers / Open threads
- NONE - TASK-0009 is fully unblocked
