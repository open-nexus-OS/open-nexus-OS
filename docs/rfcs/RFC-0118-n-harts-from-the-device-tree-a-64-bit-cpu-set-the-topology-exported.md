---
schema: 1
id: RFC-0118
title: "n harts from the device tree: a 64-bit CPU set, per-hart state sized at boot, the topology exported"
status: draft
owners: ["@kernel-team"]
created: 2026-11-10
subsystem: kernel
tasks: [TASK-0330, TASK-0365]
depends_on: [RFC-0021, RFC-0098]
links:
  - label: "Lock decomposition"
    target: docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md
  - label: "Placement today"
    target: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md
  - label: "Steal and deadlines per hart"
    target: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0118: n harts from the device tree: a 64-bit CPU set, per-hart state sized at boot, the topology exported

## Why

The kernel runs `MAX_CPUS = 4` harts. The reference board has eight in two clusters; the second
cluster stays stopped and unnamed. The constant sizes 21 statics, two tables inside the
scheduler and the task table, and one derived slot count (95 uses in 17 files, counted
2026-11-10 in `docs/board/measurements/2026-11-10-smp-baseline/`), an affinity mask is a `u8`, and userspace names harts by literal (`0b0001`, `0b1110`, `0xF`,
`MAX_WORKERS = 4`). A bigger constant would repeat the fault at the next board. The hart count
is a fact of the device tree (RFC-0098: the tree is the one hardware truth); this RFC makes the
kernel and userspace read it there.

## Scope

- **Owns:** the logical CPU index, the `CpuSet` ABI, how per-hart state is sized, the topology a
  program may ask the kernel for, the PLIC context and interrupt route per hart, the start of a
  secondary hart.
- **Does not own:** the locks (RFC-0102), the scheduler's policy and time rights (TASK-0363's
  RFC), which role runs where (TASK-0365).

## Contract (normative)

### Logical CPUs

- The kernel numbers the harts it runs densely from 0: **CPU 0 is the boot hart**, the others
  follow in the tree's order of enabled `/cpus/cpu@*` nodes. A hart id is never an index;
  `hart_of(cpu)` and `cpu_of(hart)` are the two maps, built once at boot.
- `CpuSet::BITS = 64` (`nexus-sched`) is the one compile-time ceiling. A tree naming more enabled harts than
  that starts the first 64 and says so (`KGATE: smp harts beyond the cpu set (named=N run=64)`).
  `MAX_CPUS` is deleted; its name is retired (`scripts/check-retired-names.sh`).

### `CpuSet`

- `CpuSet(u64)`: bit `n` names logical CPU `n`. It replaces the `u8` affinity mask in the
  kernel, in `nexus-abi` and in every caller. On the wire it is the one register the sched
  syscall already carries (the argument and the result are `usize`); no number changes.
- A set is **valid** when it names at least one online CPU and no CPU the kernel does not run.
  An invalid set is refused with `EINVAL`, never clamped silently: a program that names a hart
  that is not there has a bug the topology would have prevented.
- A spawn inherits its parent's set (unchanged). A hart going offline is not part of this RFC.

### Per-hart state

- Everything sized by the hart count is allocated once, before secondaries start, from a boot
  slab sized by `hart_count()`: run queues and the current-task table, resched and IPI counters,
  wake hints, timer and deadline shadows, the steal gate, the interrupt stash, PLIC context
  numbers, console line buffers, TLB mailboxes, the secondary stacks (from the frame pool, each
  with an unmapped guard page below it).
- `HART_LOCALS` stays a static array of `CpuSet::BITS` entries: `trap.S` finds a hart's block by
  address before any allocator runs. The boot hart's entries that a trap may touch before the
  slab exists (its interrupt stash, its console line) are the static block's own.
- No structure is indexed by a hart id, and no loop runs to a constant: loops run to
  `hart_count()`.

### Topology

A program asks the kernel; it never parses the tree for it and never assumes it. One operation
of the sched syscall, registers only (no user memory is read or written):

| `what` | argument | result |
|---|---|---|
| `CPUS` | — | the number of logical CPUs the kernel runs |
| `ONLINE` | — | the `CpuSet` of CPUs that came up (trap vector installed, about to enter their scheduling loop) — the set an affinity set is admitted against |
| `CLUSTERS` | — | the number of clusters (`/cpus/cpu-map`; 1 when the tree names none) |
| `CLUSTER_SET` | cluster index | the `CpuSet` of that cluster; `EINVAL` beyond the count |
| `HART_OF` | CPU index | the hart id (for diagnostics and logs); `EINVAL` beyond the count |

Any task may ask: the answer carries no authority. Changing another task's set stays the right
of the QoS administrators (ADR-0049). `nexus_abi::sched::topology()` returns the whole picture
as one value; init, execd and the workpool (pinched through it) derive their sets from it. Until
TASK-0365 replaces pinning by roles and time rights, the roles of ADR-0049 are derived, not
written (`nexus_sched::Placement`): the display chain's set is `{CPU 0}`, the background's set
is `ONLINE \ {CPU 0}` (and all of `ONLINE` on one hart), a program that may run anywhere gets
`ONLINE`. The operation is number 7 of the sched syscall (`nexus_sched::TOPOLOGY_OP`); a reader
refuses answers that describe no machine (no CPU, more than the set holds, CPU 0 offline, a CPU
online that is not run).

### Interrupts

- A hart's PLIC context is the one the tree's `interrupts-extended` of the controller names for
  its supervisor external interrupt — not `2·hart + 1`. Every online hart's context is
  initialised (threshold, all sources disabled).
- A bound interrupt is enabled in exactly one context: its driver thread's home CPU's. When the
  home changes, the kernel moves the enable (off in the old context, on in the new) at the
  thread's next dispatch; a claim is completed in the context it was claimed in.
- The console's line stays the kernel's (RFC-0107) and stays on CPU 0.

### Starting a hart

- A secondary is started through SBI HSM at the **physical** address of the entry stub, on the
  first try and on every retry (`retry_missing_harts` passed a virtual address).
- The stub is handed the physical address of its CPU's entry record (`a1`): it marks the record
  before it has a stack or a translation, and takes its stack and its logical CPU from the
  record — never from the hart id, which indexes nothing. A start the firmware refuses is named
  and the next hart is still tried.
- A hart the tree disables is not a CPU and is never started. A hart that does not come online after
  the bounded retries degrades the boot loudly (`KGATE: smp bringup DEGRADED expected=… got=…`),
  as today; the kernel runs the harts it has.
- The boot hart is whichever hart the firmware entered on; nothing assumes hart 0.

## Invariants

| Invariant | Proof |
|---|---|
| a set naming no online CPU is refused | `test_reject_cpu_set_without_an_online_cpu` |
| a set naming a CPU the kernel does not run is refused | `test_reject_cpu_set_beyond_the_harts` |
| the empty set is refused | `test_reject_empty_cpu_set` |
| a topology index beyond its count is refused | `test_reject_topology_index_beyond_count` |
| only a QoS administrator changes another task's set | `test_reject_cpu_set_for_another_task_without_the_right` (kept) |
| the topology equals the tree's | host goldens against `bpi-f3.dtb` (8 CPUs, 2 clusters of 4) and `virt.dtb` |
| CPU 0 is the boot hart whatever its hart id | host test with a tree whose boot hart is not 0 |
| a steal never crosses a task's set | `KSELFTEST: test_reject_steal_against_affinity` (kept) |
| no per-hart static is sized by a constant | the retired-name gate for `MAX_CPUS` |
| every per-hart table holds exactly the machine's CPUs, and none was forgotten at sizing | `KSELFTEST: per-hart state ok (harts= unsized=0)` on every lane; `sync::hart_table` host tests |
| a reader of the topology refuses answers that describe no machine | `test_reject_answers_that_describe_no_machine` |
| a role's set is one the kernel admits on every machine | `placement_ssot_roles_are_sane` (one CPU, two, eight with one lost) |

## Failure model

- More harts than `CpuSet::BITS`: the rest are not started, one line says so.
- The boot slab cannot be allocated: the boot stops with its name (`KPANIC: smp slab`); there is
  no path that runs with a partial per-hart state.
- A hart lost at bring-up: degraded boot, the lost hart is absent from `ONLINE`, sets naming
  only it are refused.
- A tree without `cpu-map`: one cluster holding every CPU.

## Security

A CPU set is not an authority and the topology is not a secret; the right to place another task
stays with the administrators the policy names. The sched syscall's new operation reads no user
memory, so it adds no copy path.

## Proof

```bash
just test-kernel                      # CpuSet rules, cpu_of/hart_of, slab sizing
cargo test -p nexus-fdt               # topology goldens
just ci-os-smp1 && just ci-os-smp     # one hart and two harts (per-hart state, the roles applied)
SMP=8 … ×10                           # 0 lost harts (TASK-0330 P1's gate)
```

Board: `KSELFTEST: smp online ok harts=8 clusters=2`, an interrupt served off CPU 0
(`KSELFTEST: irq off boot hart ok`), `[PASS] board-visible`.

## Alternatives considered

- **`MAX_CPUS = 8`.** Rejected: the same constant, the same statics, wrong again at the
  next board; the operator's decision of 2026-10-06 is the end state.
- **Hart ids as indices.** Rejected: hart ids are sparse on real SoCs and the boot hart is not
  always 0.
- **Userspace reads the topology from the tree.** Rejected: which CPUs are online and which
  logical index a hart has are kernel facts; two readers of one truth drift.
- **A wider or variable-length set.** Rejected until a machine needs it: one register keeps the
  operation free of user memory; the width is the one named ceiling.
