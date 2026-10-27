<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0085: Memory pressure is an event — the pool tells ONE sink its level, the levels are watermarks the machine sets at boot, and a purgeable page goes first

- Status: Accepted (2026-10-27; the proof is both QEMU lanes and board cycle 73 of TASK-0333 6e — the kernel half built in 6e-0, proven by `KSELFTEST: mem watermarks ok (…)` and `KSELFTEST: mem pressure notify ok (…)`; the purgeable half built in 6e-3 and proven by `MM: pressure purge level=warn pages=4` → `KSELFTEST: vmo purge ok (pages=4 purged=4 relocked=purged reject=bound,eager,mapped)` on both QEMU lanes and on the board's two banks in cycle 73)
- Date: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6e-0, 6e-3)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md`
    (§3 accounting and pressure, §4 purgeable objects — this decision fixes both)
  - Related ADRs: `docs/adr/0073-…` (death is an event: the exit funnel's frame into a bound
    endpoint, the shape this notification takes), `docs/adr/0077-…` (commit on touch: the pages a
    purge may take are the committed ones of a lazy object), `docs/adr/0078-…` (one pool with the
    GPU: a GPU-bound page is never purged), `docs/adr/0086-…` (memd: the one service that binds
    the sink and decides)

## Context

Through 6a–6d the kernel learned to free what dies, to commit on touch, to share code and to
freeze a hidden app. It still cannot say *how tight memory is*: `mm_stats` (syscall 61) answers
the pool's free frames to whoever asks, and metricsd asks once at readiness. Nothing in the
system reacts to a pool running low before an allocation is refused — and a refusal is the one
event the kernel does emit (`MM: frames exhausted`), too late for anything but a log line. The
phones this OS is measured against turned memory pressure into an event a decade ago: the
kernel tells one daemon the level, the daemon shrinks caches at the first level and kills by
band at the next, and no app ever sees an allocation fail. Measured 2026-10-27 (board cycle 72b):
1 043 883 frames, 996 710 free with six apps and the desktop up; QEMU: 77 261 frames, 43 383
free. The same system must know "low" on both.

Three forks were on the table. (1) Where the thresholds come from: fixed byte counts (wrong on
one of the two machines), fractions of the pool's total (counts frames the firmware and the
kernel image hold), or fractions of what each bank has *free when the pool is built* (what the
system can actually spend). (2) Who is told: every service that asks (a poll), every service
that binds (a fan-out in the kernel), or one sink that fans out in userspace. (3) Whether the
level is a bank's or the pool's: the buddies fill the banks in tree order, so a per-bank level
would alarm on the first bank with the second still free.

## Decision

- **The watermarks are the machine's.** When the frame pool is built, each bank's free frames
  contribute 25 % (`warn`), 10 % (`critical`) and 5 % (`emergency`) to three pool-wide
  watermarks; each is floored (1024 / 512 / 256 frames) so the ordering `warn > critical >
  emergency > 0` holds on any pool. The level is the pool's: `free ≤ watermark` enters a level
  at once; leaving it needs free frames above the watermark by an eighth (hysteresis), so a
  boundary never flaps into a storm of notifications. `KSELFTEST: mem watermarks ok (total=
  free= warn= crit= emerg= level=)` at the late fence proves the ordering on every boot.
- **Pressure is an event into ONE sink.** `SYSCALL_MEM_PRESSURE_BIND` (67) binds an endpoint
  the caller holds a SEND capability to — the shape of `child_exit_bind` (62). Every level
  change writes one `OP_MEMORY_PRESSURE {level, free, total}` frame (op 0x71, 18 bytes) into
  it and wakes the waiter; the bind itself delivers the level of the moment, so the binder
  never has to ask. The pool observes its free frames after every allocation and free with its
  lock dropped; the frame is delivered where the kernel already delivers its events — the
  syscall epilogue, the user fault's fill, the timer tick, the idle loop — never from inside
  the pool. The first binder holds the sink until its endpoint dies; another process's bind is
  refused (`EPERM`); the holder may re-bind or clear. The lifecycle authority of memory is
  `memd` (ADR-0086), started by init before any app runs; the kernel does not know its name —
  it knows that a sink is held.
- **The record says what is shared and what may go.** `mm_stats` v4 adds the caller's shared
  bytes (resident bytes over objects another space maps too: a shared image's text, the
  atlas), its purgeable bytes, the level, the three watermarks and the pages purged since boot.
  `KINIT: mm space … shared=` names it per space at the fence.
- **A purgeable page goes first (6e-3).** A lazy object's owner may mark it purgeable; at
  `warn` and above the kernel may drop its committed clean pages, and the owner learns it as a
  status on its next lock — never as a fault that kills. A page bound into a GPU context or
  told to a device (`vmo_runs`) is never purged: the GPU and the device would read freed
  frames.
- **Out of scope here**: who kills whom (memd, ADR-0086), the compressor and the swap (6f,
  RFC-0100 §5), a per-bank level (rejected above), a kernel-side fan-out to many binders.

## Consequences

- **Positive**: one mechanism tells the system "low" before the first refusal; the thresholds
  are right on a 320 MiB QEMU and a 4 GiB board without a configuration file; a service that
  missed a change reads the truth of the moment in the frame it does get (`free`, `total`) or
  in `mm_stats`; the kernel's event shapes stay one (a frame into a bound endpoint).
- **Negative / accepted cost**: a seventh kernel event path to keep delivering (the four
  dispatch sites); the first-binder rule is weaker than a minted capability — it relies on init
  starting memd before any app, which init does by construction (apps are spawned by execd on
  abilitymgr's order, after the service fleet); `MmStats` consumers re-build (v3 → v4, metricsd
  and the selftest probes).
- **Follow-ups**: ADR-0086 (memd, the bands, the kill through the parent); 6e-3 (the purgeable
  mark and lock, gpud's caches purgeable); 6f (`lowmem` lane: a real level change under a
  128 MiB QEMU, the compressor, the swap); a minted pressure capability if a second binder ever
  has a legitimate claim.

## Alternatives considered

- Polling `mm_stats` on a timer in memd — a poll against a clock, the pattern this repo retired
  (ADR-0073, `check-wait-not-poll`); a 100 ms period misses a 50 ms burst and costs a wake per
  period for nothing.
- Fixed thresholds (e.g. 64 MiB / 16 MiB) — right on one machine, wrong on the other; a
  configuration file would be a second source of truth about the machine the tree already
  describes.
- Fractions of the pool's total — counts the frames the tree reserved and the kernel excluded,
  which the system can never spend; the boot-time free total is what it can.
- A per-bank level — the buddies fill banks in tree order; bank 0 at `emergency` with bank 1
  untouched is not pressure.
- A fan-out in the kernel to every binder — a table, a quota per binder, a policy about who may
  bind: userspace's job (memd's), and one frame into one sink is the kernel's whole duty.
- A capability minted by init for the sink — the right end state for a second legitimate
  binder; today there is one, and the first-binder rule costs no new capability kind.
