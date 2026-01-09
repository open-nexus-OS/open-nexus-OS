# Status Board (Dynamic): Draft / In Progress / Done

This file is a **status view** over tasks, optimized for daily work:

- a **small, maintained** “board” for the active set,
- without forcing task filenames to be sorted,
- and without duplicating per-task DoD/proofs.

**Source of truth remains** each `tasks/TASK-*.md` file.

## Best-practice notes (what enterprises do)

- **Board rules** (maybe Jira/ADO). In-repo equivalents work well for OS repos, as long as:
  - the board stays small (active set + next),
  - and tasks remain authoritative for scope/proofs.
- **Percent complete is always approximate**. Enterprises often prefer:
  - checklist items tied to DoD, or
  - explicit phases like “Spec → Implement → Proof → Harden”.
  If you want a percentage anyway, keep it **rough** and tied to proofs.

## How to use this board

- **Status** is the task header `status:` (Draft / In Progress / Done).
- **Progress** is a rough estimate: `0% / 25% / 50% / 75% / 90% / 100%`.
  - `90%` means: implementation done, proof/hardening is the remaining work.
- **Blocked-by** must reference task IDs (or a short explicit reason).

## Active Set (Now)

| Lane    | Task                                     | Status      | Progress | Blocked-by                      | Next proof / next action                             |
| ------- | ---------------------------------------- | ----------- | -------- | ------------------------------- | ---------------------------------------------------- |
| KERNEL  | TASK-0011 Kernel simplification phase A  | In Progress |          |                                 | Finish restructuring slice; keep proofs green        |
| KERNEL  | TASK-0011B Kernel Rust idioms pre-SMP    | In Progress |          |                                 | Finalize ownership/newtype conventions; link pilots  |
| KERNEL  | TASK-0281 Kernel newtypes v1c            | Draft       |          | TASK-0011B                      | Extend newtype coverage + tests                      |
| KERNEL  | TASK-0282 Typed capability rights v1     | Draft       |          | TASK-0011B                      | Pilot typed caps in one subsystem                    |
| KERNEL  | TASK-0283 Per-CPU ownership wrapper v1   | Draft       |          | TASK-0012                       | Introduce `PerCpu<T>` and adopt in scheduler/mailbox |
| DRIVERS | ADR-0018 DriverKit ABI policy            | Proposed    | 100%     |                                 | Review + accept ADR                                  |
| DRIVERS | TASK-0280 DriverKit v1 core contracts    | Draft       |          | TASK-0010, TASK-0031, TASK-0013 | Host-first crate + deterministic tests               |
| DRIVERS | TASK-0284 DMA buffer ownership prototype | Draft       |          | TASK-0031                       | Host-first `DmaBuffer` + fence ownership tests       |

## Next (Queued)

| Lane     | Task                                                    | Status      | Progress | Blocked-by            | Why it’s next                                |
| -------- | ------------------------------------------------------- | ----------- | -------- | --------------------- | -------------------------------------------- |
| BRING-UP | TASK-0244 RV virt v1.0a host DTB + SBI shim             | In Progress |          |                       | Required for stable bring-up path            |
| BRING-UP | TASK-0245 RV virt v1.0b OS UART/PLIC/TIMER + uartd      | In Progress |          | TASK-0244             | First OS kernel↔userspace driver integration |
| BRING-UP | TASK-0247 RV virt v1.1b OS SMP + virtioblkd + packagefs | In Progress |          | TASK-0245, TASK-0012  | SMP bring-up + storage path                  |
| KERNEL   | TASK-0012 SMP v1 per-CPU runqueues + IPIs               | In Progress |          | TASK-0011, TASK-0011B | SMP foundation                               |
| KERNEL   | TASK-0013 QoS ABI + timed coalescing                    | In Progress |          |                       | Needed for latency/budget hints              |
| KERNEL   | TASK-0042 SMP v2 affinity + QoS budgets                 | In Progress |          | TASK-0012, TASK-0013  | Supports device-class scheduling             |

## Done (recent)

| Task                                 | Notes                         |
| ------------------------------------ | ----------------------------- |
| TASK-0001 Runtime roles & boundaries | Single-authority model locked |
| TASK-0002 Userspace VFS proof        | Marker-gated proof in QEMU    |

## Related

- **Implementation ordering**: `tasks/IMPLEMENTATION-ORDER.md`
- **Kernel inventory (security + comparisons)**: `docs/architecture/KERNEL-TASK-INVENTORY.md`
