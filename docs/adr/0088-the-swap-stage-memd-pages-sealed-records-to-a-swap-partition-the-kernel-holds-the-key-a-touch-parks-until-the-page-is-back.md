<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0088: The swap stage — memd pages sealed records to a `swap` partition, the kernel holds the key, a touch parks until the page is back

- Status: Accepted (2026-10-28, TASK-0333 6f-2; both QEMU lanes + board cycle 76 — the boot
  proof `KSELFTEST: swap ok` on the board's harts, the whole chain on the usb-visible lane)
- Date: 2026-10-28
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6f-2)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md`
    (§5 the backing stages — this decision is its swap stage), `docs/rfcs/RFC-0089-…` (§2 the disk
    layout the `swap` partition joins)
  - Related ADRs: `docs/adr/0087-…` (the pool whose records the swap stage moves out),
    `docs/adr/0086-…` (memd, the memory authority — now its pager too), `docs/adr/0085-…` (the
    pressure sink the page-in requests ride), `docs/adr/0044-…` (blkd, the one owner of the disk)

## Context

6f-1 squeezes a frozen app's private memory into records in kernel RAM: an app-host's 1.8 MB of
heap and stack became 468 KB on the usb-visible lane. Under continued pressure RFC-0100 §5 moves
those records to the `swap` partition before anyone is killed. Three facts shaped the decision:

- the records are app memory — anything an app holds, keys included — and the eMMC keeps what is
  written to it across a reboot and out of the board;
- a page the app touches while its record is on disk cannot be brought back synchronously the
  way 6f-1 inflates one: the bytes are behind blkd, a disk and a service round trip;
- the board's entropy source fails today (`SELFTEST: rng entropy FAIL`); QEMU's works.

## Decision

- **The kernel seals, memd only moves ciphertext.** A record leaves the kernel sealed with
  ChaCha20-Poly1305 under a per-boot key: the nonce a kernel-wide counter (never reused within a
  boot), the associated data the page's identity (object, page index) — a record of one page
  never opens as another, an older record of the same page never opens again. The key is
  seeded ONCE, by the RNG service alone (syscall 70, its kernel service id); without a key the
  kernel refuses every export: no entropy, no swap. The RNG service gets its entropy device only
  after it starts, so the key is seeded on demand: the pager asks the RNG service (memd's kernel
  identity alone) when a swap is first due, and the key goes from the RNG service into the
  kernel, never through the pager.
- **Two phases, nothing lost.** `swap_export` (71) seals up to 16 records of a process into the
  pager's buffer, each with the counter it was sealed under; the pages stay records. The pager
  writes them to its slots and `swap_commit`s (72) each, naming that counter: only then is the
  page swapped and its record released. A touch between the two inflates the record and cancels
  the export, a second export supersedes an uncommitted first — a commit that names any other
  export is refused and its slot freed, so no slot ever holds a record that would not open.
- **A touch parks until the page is back.** A user fault on a swapped page parks the faulting
  thread (its instruction re-executes); a syscall that touches one parks its caller (the syscall
  re-executes, the way `Reschedule` does); an IPC call already sent makes its reply buffer
  writable again before the reply leaves its queue. The kernel asks the pager on the pressure
  sink — `OP_PAGE_IN {token, slot}`, once per page however many threads wait, queued until the
  sink takes it — and the pager hands the sealed record back (`swap_page_in`, 73, naming the
  slot): the kernel opens it, inflates it into a fresh frame and wakes every thread parked on it.
  An answer for a page no longer swapped in that slot wakes its waiters to retry; a record that
  does not open is never entered, the threads parked on it are killed.
- **The pager is the sink's holder.** The export, commit and page-in syscalls are refused for
  any process but the one holding the pressure sink (memd, ADR-0085/0086), and an export of the
  pager itself; the key's seeding for anyone but the RNG service. A dying or purged object's
  swapped pages are told to the pager as `OP_SWAP_DROP {slots}` (32 a frame), so a slot is freed
  exactly once.
- **The disk.** A `swap` partition (64 MiB, 16 384 slots) is appended to the layout (RFC-0089 §2);
  blkd grants it to memd alone, read and write. One 4 KiB slot holds one sealed record and its
  length. Nothing on it outlives the boot: it is never flashed.
- **The policy.** memd at `critical`: squeeze a frozen app, then swap out a squeezed frozen app,
  then kill; `emergency` kills at once. When a swapped app is thawed, memd pages its records
  back before the app asks for them.
- **Out of this decision:** swapping uncompressed pages, swapping a service's memory, compacting
  the partition, swap on the board until its entropy source works.

## Consequences

- **Positive**: a frozen app's memory can leave RAM entirely and come back intact; neither the
  pager nor the disk ever holds plaintext; an app whose pages are still on disk resumes — slowly
  on its first touches — instead of being killed.
- **Negative / accepted cost**: the kernel links an AEAD implementation (pure Rust, no
  dependencies outside the workspace's audited set); a fault can now park a thread on a service
  round trip and a disk read; the pager and blkd are on the way back of every swapped page — a
  pager that stops answering stalls the threads it should serve (the kernel never times them
  out); the pager holds a route to the RNG service for the one seed request; the board does not
  swap until it has an entropy device (its tree names none today).
- **Follow-ups**: the `lowmem` lane gives the kernel-sourced path a real `critical` (6f-3); the
  board's entropy source (its own task); swap at rest beyond one boot is never a goal — the key
  dies with the boot.

## Alternatives considered

- **memd encrypts** (the kernel keeps a digest per page for integrity). Rejected: the pager would
  read every swapped app's memory in clear; with the kernel sealing, memd's compromise does not
  expose app memory.
- **No encryption, the partition zeroed at boot.** Rejected: the eMMC can be read without
  booting it.
- **One phase (the kernel releases the record at export).** Rejected: a failed write, or a pager
  that dies before writing, would lose the pages.
- **No swap at all, as the mobile systems do (compression and kill).** Considered: RFC-0100 §5
  keeps the stage for the desktop's larger working sets; the flash wear is bounded because swap
  only runs at `critical`, and the policy can always stop at the pool.
