# ADR-0039: Device-class driver architecture — bus-HAL + DriverKit + a thin device shim

- Status: Accepted (foundation landed: `nexus-virtio` bus-HAL + `nexus-driverkit` submit/fence/budget; the GPU stack is the worked reference. Gate 4 of the gfx/driver idealstruktur track.)
- Created: 2026-06-26
- Builds on: ADR-0018 (DriverKit ABI versioning), ADR-0032 (GPU command ring + pipelined present), ADR-0033 (soft-real-time spine), ADR-0038 (display wire SSOT).
- Tracks: `tasks/TRACK-DRIVERS-ACCELERATORS.md`, `tasks/TRACK-NEXUSGFX-SDK.md`.
- Code: `source/libs/nexus-virtio`, `source/libs/nexus-driverkit`, `userspace/nexus-gfx`, `source/drivers/gpud`, `source/drivers/net/virtio`.

## Context

A device-class service (GPU / NPU / VPU / audio / camera-ISP / storage / net) had no shared shape:
every virtio driver hand-rolled the same virtio-mmio register map + feature negotiation + queue
setup, and gpud's submit ring (ADR-0032) was a one-off. The vision (TRACK-DRIVERS-ACCELERATORS) is
the opposite: the *same* "submit + fence + buffers + budgets" plus the *same* bus bring-up should be
shared, so a real device driver shrinks to the parts that are genuinely device-specific — and an SDK
(NexusGfx, NexusMedia, NexusInfer) sits on top. This ADR records the **layering** that the landed
work (Gates 1–3) now realizes, as the template every future device class follows.

## Decision

A device-class driver is composed of **three shared layers plus one thin device shim**:

```
Apps / SystemUI / windowd            ── clients
        │  SDK (NexusGfx / NexusMedia / NexusInfer): explicit, capability-first API
        ▼
SDK crate            ── command/resource vocabulary (SSOT) + reference CPU executor
        │  one wire codec (e.g. nexus-gfx CommittedBuffer); zero-copy bulk via VMO cap-move
        ▼
device-class service ── implements the SDK backend: device command encode/validate + the device shim
        │  nexus-driverkit: SubmitRing + completion fence + BufferBudget + QoS   (cross-device)
        ▼
bus HAL              ── nexus-virtio (virtio-mmio): probe/negotiate/queue/notify/IRQ + ring structs
        ▼                (other buses — PCIe, platform — get their own HAL behind the same shape)
kernel               ── cap-gated MMIO/IRQ/DMA, VMOs, IPC endpoints, timeline fence, waitset
```

**What is shared (libraries, versioned per ADR-0018):**

- **Bus bring-up** — `nexus-virtio` (`VirtioMmio<B: Bus>`): the register map, the
  reset→ACK→DRIVER→features→FEATURES_OK→queue→DRIVER_OK handshake, split-virtqueue ring structs, and
  queue programming. Generic over `nexus_hal::Bus`, so it is `forbid(unsafe_code)`; the raw MMIO is a
  tiny per-driver `Bus` impl. A non-virtio bus (PCIe, platform) gets a sibling HAL of the same shape.
- **Submit / sync / budgets** — `nexus-driverkit`: the bounded in-flight `SubmitRing` (backpressure +
  completion counter a timeline fence mirrors), `BufferBudget`, and `Qos`. Pure, `no_std`, alloc-free.
- **The SDK** — the command/resource vocabulary + a reference CPU executor (e.g. nexus-gfx's canonical
  rasterizer, Gate 1), and one wire codec (Gate 2). Bulk payloads never cross IPC — they live in a
  shared VMO moved by capability.

**What stays device-specific (the thin shim — ideally the only new code per device):**

- device id + config registers,
- command-stream encoding / validation,
- firmware protocol (if the device needs one),
- reset / recovery.

**The GPU stack is the worked reference**: NexusGfx (SDK) → gpud (`GfxBackend` impl: virtio-gpu
encode + virgl) → nexus-driverkit (`CtrlQueue` on `SubmitRing` + present fence) → nexus-virtio (bus).
net-virtio is the minimal reference (transport + a userspace data plane, no ring).

## Per-class mapping (the template applied)

| Class | Bus HAL | DriverKit usage | Device shim (the thin part) | SDK |
|---|---|---|---|---|
| **GPU** (gpud) | nexus-virtio | present `SubmitRing` + present fence + VMO budget | virtio-gpu cmd encode + virgl | NexusGfx |
| **Storage** (virtio-blk) | nexus-virtio | request ring + completion fence + in-flight budget | block request encode | — |
| **Net** (virtio-net) | nexus-virtio | (data plane in userspace) | rx/tx queue glue | NexusNet |
| **Audio** (future) | nexus-virtio (virtio-sound) | period/buffer ring + **timeline fence for A/V sync** + buffer budget | period/format config | NexusMedia |
| **NPU** (future) | bus HAL (virtio / PCIe) | inference submit ring + completion fence + model/tensor budget + **power-profile QoS** | command encode/validate | NexusInfer |
| **Camera/ISP** (future) | bus HAL | frame ring + **per-frame deadline QoS** + VMO frame budget + privacy gates | sensor/ISP config | NexusMedia |

The mobile/tile-aware stance (TRACK-NEXUSGFX-SDK) is preserved at the SDK layer (pass locality,
bandwidth-first), not hard-coded into any vendor API; real GPU/NPU backends plug in behind the SDK
without changing this layering.

## Consequences

- A new device-class driver is "pick a bus HAL + use DriverKit + write the device shim" — the
  boilerplate (register map, negotiation, ring/fence/budget plumbing) is no longer re-written.
- The boundary is narrow and versioned (ADR-0018), so SDKs and drivers evolve independently.
- Landed today: nexus-virtio (Gate 3), nexus-driverkit (ADR-0033), nexus-gfx SSOT rasterizer + wire
  (Gates 1–2). Pending migrations onto the HAL (storage, then the booted rng/input/gpud) are
  incremental and boot-gated; audio/NPU/camera classes are future tasks under the tracks above.
