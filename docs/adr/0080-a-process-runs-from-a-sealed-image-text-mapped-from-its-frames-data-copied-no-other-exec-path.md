# ADR-0080: A process runs from a sealed image — its text and read-only data mapped from the image's own frames, its data copied, and there is no other exec path

- Status: Accepted (board cycle 70, `board--2026-10-26T10-32-12`: H1–H4 of `docs/board/measurements/2026-10-26-sealed-images/`)
- Date: 2026-10-26
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6c-1b)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md` (§2 the sealed kind, §4 seals), `docs/rfcs/RFC-0089-…` §12 / ADR-0060 (the verified volume, init the spawner), RFC-0080 (the read-only alias precedent)
  - Related ADRs: `docs/adr/0077-…` (lazy objects), `docs/adr/0079-…` (cache maintenance is a right)

## Context

`exec` takes a user pointer and a length (syscalls 13 and 17) and copies every file byte of
every segment into fresh frames: N app-hosts hold N copies of 1.93 MiB of identical code and
read-only data; a volume service is held twice (init keeps the DMA object mapped for respawn,
the kernel copies it at spawn); the bytes are read from memory a block device was told to
write into, after the hash was taken. Measured 2026-10-26: every shipped ELF links with
`-z separate-loadable-segments` at +0.1–4 % size — each `PT_LOAD` begins on its own file page,
`p_offset ≡ p_vaddr (mod 4096)`, no file page is shared between segments — and the data
segments are 8–32 bytes (one page each). Launch time is not the copy (6a's H6, refuted on both
lanes): this decision buys memory and an immutable origin, not speed.

## Decision

- **A VMO can be sealed, once, by a holder of its `Vmo` capability** (`vmo_seal`, syscall 64).
  Sealing is refused while any address space maps the object writably, when a device was ever
  named its runs (`vmo_runs`), and for a lazy or fixed object. Sealed, the object refuses
  `vmo_write`, a writable `vm_map` and `vmo_runs` for the rest of its life; reads, read-only maps
  and read-only aliases stay. A sealed object dies by the lifetime rule like any other (RFC-0100
  §1).
- **`exec_vmo` (syscall 63) is the only way a process is made from an image.** It takes a sealed
  object's capability (`Vmo` or `VmoRo`, MAP), the stack size, the global pointer and the
  service name. Its `PT_LOAD`s must begin on page-aligned file offsets congruent with their
  virtual addresses and must not share a file page (the kernel refuses otherwise): a
  read-only or executable segment is **mapped from the image's frames** — the same frames in
  every process that runs it — a writable segment's file pages are **copied** into frames the
  process owns, and its `.bss` and the stack stay lazy objects (ADR-0077). Only this path maps
  a page executable, and only from a sealed object. Syscalls 13 and 17 are deleted and their
  numbers retired.
- **An image is sealed before it is verified, and verified before it runs.** bundlemgrd reads a
  bundle from the volume into a bounce object the block device may write, copies it into the
  spawner's image object, seals it, and only then hashes the sealed bytes against the index
  digest; init execs from the sealed object and keeps its capability for respawn — no mapping,
  no second copy. An embedded image (init's CORE services, execd's app-host and probes) is
  copied once from the binary into a sealed object and reused for every spawn.
- **Out of scope**: copy-on-write (the data segments are one page: a copy is cheaper than a
  fault), dropping clean code pages under pressure (6e/6f), the boot image that replaces the
  nesting of binaries (6c-2), a per-page signature check on fault.

## Consequences

- **Positive**: one loader; code shared by construction (the second app-host costs its private
  page, its stack and its heap); a volume service held once; the hashed bytes are the bytes that
  run (the seal is between them and every writer); the W^X floor becomes "executable only from
  a sealed object".
- **Negative / accepted cost**: every image needs page-aligned segments (the linker flag in
  `.cargo/config.toml`, a layout check at build time in one shared crate, the kernel's refusal
  as the last gate); the hand-made probe ELFs grow by a page; an embedded CORE service exists
  twice until 6c-2 (as today: in the binary and in memory); one more trap-free copy per bundle
  read in bundlemgrd (bounce → image, ~1 ms per MB).
- **Follow-ups**: 6c-2 (the boot image); the `rss` and `mappers` lines in the measurement
  folder; `execd`'s image cache is the page cache of RFC-0100 §2 for the embedded images.

## Alternatives considered

- **Map from a read-only alias with a new EXEC right** (Fuchsia's VMEX): a right can be cloned
  anywhere; the seal is a property of the object, not of who holds it — simpler to reason about
  and refusable at the object.
- **Keep pointer+length exec and copy-on-write the pages**: still a copy per process of the
  read-only pages on the first write that never comes; and the copy source stays a user buffer
  a device can write.
- **Exec straight from the DMA object**: the block device has the object's runs and can write
  them after the hash — rejected; the bounce copy is the price of an immutable origin.
- **A build-time gate only, no kernel refusal**: an image not built by this tree would run with
  misaligned segments mapped shared — rejected; the kernel refuses, the build gate reports early.
