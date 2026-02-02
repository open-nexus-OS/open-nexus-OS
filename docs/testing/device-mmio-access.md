---
title: Device MMIO access — tests & future extensions
---

## Purpose

This document describes **tests that are valuable today** for the device-MMIO access model and can be
**extended later** as more device functionality (virtqueues, DMA, IRQ) becomes testable.

Scope: the v1 MMIO foundation contract defined by `RFC-0017` / `TASK-0010`.

## What we test today (high value, low flake risk)

### 1) Kernel safety invariants (must never regress)

These tests protect the kernel/user boundary from accidental widening:

- **Capability gating**: mapping without a `DeviceMmio` cap is rejected
- **Kind confusion**: mapping with the wrong cap kind is rejected
- **Bounds**: mapping outside the cap window is rejected
- **Rights**: mapping without `Rights::MAP` is rejected
- **W^X floor**: MMIO mappings are **never executable**
- **Page granularity**:
  - VA must be page-aligned
  - offset must be page-aligned
- **No silent overwrite**: mapping the same VA twice fails deterministically (overlap)

Where: kernel syscall tests in `source/kernel/neuron/src/syscall/api.rs` (MMIO reject suite).

### 2) OS/QEMU end-to-end contract proofs (marker-gated)

These tests prove the full system wiring is real (no fake success):

- `SELFTEST: mmio map ok` — mapping works and known register reads succeed
- `rngd: mmio window mapped ok` — a designated owner service mapped its window
- `virtioblkd: mmio window mapped ok` — virtio-blk consumer path works (device present, cap distributed, mapping works)
- `SELFTEST: mmio policy deny ok` — policy deny-by-default is enforced for a non-matching MMIO capability

Where: `scripts/qemu-test.sh` marker ladder.

Fast local run:

```bash
just test-mmio
```

## What we will extend later (when capability exists)

### 1) Per-device “bring-up steps” beyond mapping

Once device frontends do more than safe reads:

- virtio feature negotiation markers
- virtqueue init markers
- bounded I/O operations (e.g. blk read of a single sector) with deterministic results

### 2) Policy negative tests at the distribution boundary

Once more policy surface exists:

- deny-by-default proofs for a deliberately unauthorized service
- structured audit assertions (presence/shape of audit records without log-grep “truth”)

### 3) DMA / IRQ follow-up (separate tasks/RFCs)

DMA and IRQ delivery are explicitly out of scope for v1 MMIO and should be tested under their
follow-up tasks/RFCs once the primitives exist.
