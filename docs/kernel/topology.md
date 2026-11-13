---
schema: 1
title: "The machine's CPUs: topology, CPU sets, per-hart state"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/core/smp/topology.rs, source/kernel/neuron/src/core/smp/hart_state.rs, source/kernel/neuron/src/core/smp/bringup.rs, source/kernel/neuron/src/sync/hart_table.rs, source/kernel/neuron/src/core/irq.rs, source/kernel/neuron/src/core/irq_route.rs, source/kernel/neuron/src/hal/plic.rs, source/libs/nexus-fdt/src/topology.rs, source/libs/nexus-sched/src, source/libs/nexus-abi/src/syscall/sched.rs, source/init/nexus-init/src/affinity.rs]
contracts: [RFC-0118, ADR-0049]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The machine's CPUs: topology, CPU sets, per-hart state

> **Purpose** — how the kernel learns which harts it runs, how it sizes its per-hart state for them, how a hart is started, and how a program asks for the machine instead of assuming it.
> **Owns** — `source/kernel/neuron/src/core/smp/topology.rs`, `source/kernel/neuron/src/sync/hart_table.rs`, `source/libs/nexus-sched/`
> **Contracts** — [RFC-0118](../rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md), [ADR-0049](../adr/0049-bkl-lockclass-and-softrt-cpu-placement.md)
> **Invariants** — no constant counts harts; `CpuSet::BITS` is the one ceiling. A hart id is never an index. CPU 0 is the boot hart. A CPU set the machine cannot run is refused, never clamped. Nothing knows a board.
> **Read next** — [smp.md](smp.md), [hart-spaces.md](hart-spaces.md), [README.md](README.md)

## Logical CPUs

The device tree's `/cpus` becomes one `Topology` (`source/libs/nexus-fdt/src/topology.rs`): the
enabled `cpu@*` nodes, the clusters of `cpu-map` (one cluster when the tree names none) and,
per hart, the interrupt controller's supervisor context from its `interrupts-extended` — the
binding, not the shape of a known tree. The kernel reads it once at boot
(`source/kernel/neuron/src/core/smp/topology.rs`):

- logical CPUs are dense from 0; **CPU 0 is the boot hart** whatever its hart id, the others
  follow in the tree's order;
- `hart_of(cpu)` and `cpu_of(hart)` are the only translation; a hart id (64 bits wide, possibly
  sparse) indexes nothing;
- a tree naming more harts than a CPU set holds runs the first 64 and says so
  (`KGATE: smp harts beyond the cpu set (named= run=)`); a tree that cannot be read is the boot
  hart alone;
- an SBI IPI to a set of CPUs is planned from the hart ids (`plan_ipi`): one call when they
  fit one mask window, one per window otherwise.

## CPU sets

`nexus_sched::CpuSet` (`source/libs/nexus-sched/src/cpu_set.rs`) is 64 bits, bit `n` = logical
CPU `n`, in the kernel, in the ABI and in every caller. `CpuSet::admit(runs, online)` is the one
rule: the empty set, a set naming a CPU the kernel does not run (even beside valid ones) and a
set naming no online CPU are refused (`EINVAL`).

## Per-hart state

Everything kept per hart is a `HartTable<T>` (`source/kernel/neuron/src/sync/hart_table.rs`):
the boot hart's entry lives in the image (a trap may need it before any allocator runs), the
others on the kernel heap, sized once by `smp::size_for_machine` before a secondary starts —
the SMP layer's records (`core/smp/hart_state.rs`, one cache-aligned record per CPU), bring-up
records, console lines, interrupt stashes, timer deadlines; run queues, the current-task table
and the spaces harts run on are boxed slices of the same length. `HART_LOCALS` alone stays a
static of `CpuSet::BITS` blocks: `trap.S` finds a hart's block by address.

- A failure to size is `KPANIC: smp slab`; no path runs with partial per-hart state.
- A table asked for the entry of a CPU the machine has before it was sized is a table the
  sizing forgot: counted, and `KSELFTEST: per-hart state ok (harts= unsized=0)` proves on every
  lane that the tables hold exactly the machine's CPUs and the count is 0.

## Starting a hart

`source/kernel/neuron/src/core/smp/bringup.rs` starts each secondary over SBI HSM at the
**physical** address of the entry stub — first try and every retry — with the physical address
of its CPU's entry record in `a1`. The stub marks the record before it has a stack or a
translation, turns the boot table on, and takes its stack and its logical CPU from the record.
A secondary's stack is frames from the pool; the lowest is a guard the kernel's half of every
address space leaves unmapped (`mm/kernel_layout.rs`). A start the firmware refuses is named and
the next hart is still tried. The gate lines are in [smp.md](smp.md); per CPU,
`KGATE: smp cpuN hart= online= asm= stage= hsm=` says which hart it is, whether it reached the
stub, how far its first Rust got and what the firmware reports, and
`KGATE: smp cpuN entered by hart … (the tree names hart …)` names a firmware that started
another hart than the one asked for.

A CPU whose hart the interrupt controller has no supervisor context for takes no external
interrupts and is named (`KINIT: cpuN has no plic context in the tree`); its registers are
never written.

## A device line

A bound line is enabled in one context: the one of the CPU its binder is homed on
(`core/irq.rs`, rules in `core/irq_route.rs`, host-tested against a controller as specified).
It follows the binder when it is pinned elsewhere or migrates — off in the old context, then
on in the new. A claim is completed where it was made and while the line is enabled there, so
a move under an outstanding claim waits for the completion. A home without a context sends the
line to the lowest online CPU that has one; with none the bind is refused. Every hart unmasks
the external interrupt and delivers what it claimed — from user mode at once, from its idle
loop otherwise. The console's line stays the kernel's, on CPU 0.
A line whose driver died is released: a claim nobody can complete any more is completed, the
line disabled, and the driver's successor binds a line that can fire.

## A program asks

Operation 7 of the sched syscall answers in registers, to any task (the answer carries no
authority): the CPUs the kernel runs, the online set, the clusters, a cluster's set, a CPU's
hart id. `nexus_abi::sched::topology()` reads the whole picture and refuses answers that
describe no machine; `online()` is the one-query form; `offers()` is what every CPU offers a
program ([float-state.md](float-state.md)).

Placement is a role, not a mask (`nexus_sched::Placement`): `Display` = CPU 0 when online,
`Background` = the other online CPUs (all of them on one hart), `Any` = the online set. init
(`source/init/nexus-init/src/affinity.rs`, `bootstrap/resume.rs`), execd's sched recipes and the
workpool (worker `i` on the `i`-th online CPU) resolve a role against the online set the kernel
reports, so the set is one the kernel admits on every machine; a refusal is a failure
(`init: affinity FAIL svc= set=`), never a reason to try a wider set. The boot summary is
`init: affinity applied n= fail= online=`.

## Proof

| Claim | Proof |
|---|---|
| the topology equals the tree's; CPU 0 is the boot hart whatever its id | `cargo test -p nexus-fdt` (the board's tree: 8 CPUs in 2 clusters of 4; `virt`; a synthetic tree with sparse 64-bit hart ids and a boot hart that is not the first node) |
| the set rules and the roles | `cargo test -p nexus-sched`, `placement_ssot_roles_are_sane` (nexus-init) |
| the IPI plan, the query answers, the hart table, a device line's route | `just test-kernel` |
| a bound line is served where its driver runs | `KSELFTEST: irq off boot hart ok (cpu= irq=)` on every multi-hart lane (`scripts/qemu-test.sh`) and the board |
| one, two and eight harts boot, none lost | `just ci-os-smp1`, `just ci-os-smp`, the eight-hart series in `docs/board/measurements/2026-11-11-n-harts/README.md` |
