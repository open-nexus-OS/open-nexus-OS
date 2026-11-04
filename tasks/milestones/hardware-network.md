---
schema: 1
id: hardware-network
title: "Hardware and network fast track"
status: active
started: 2026-09-21
goal: "The OS runs on the reference board on the desk: a picture, USB input, Sub-80 finished, then the target pictures memory, GPU and SMP in layers, and network and web after them."
sections:
  - key: B0
    title: "Tooling: one command installs the board tools"
  - key: B1
    title: "First picture: the OS boots from eMMC and shows the desktop on HDMI"
  - key: B2
    title: "USB input: keyboard and mouse over USB, QEMU first"
  - key: B3
    title: "Sub-80 remainder: every task below 0080 done"
  - key: L
    title: "Block 4 in layers: memory, GPU and SMP, each layer ending in a board gate"
  - key: NET
    title: "Network: WiFi and Ethernet, DHCP and DNS, TLS, the clock from the internet"
  - key: WEB
    title: "Web: a std target, an isolated engine component, the browser"
steps:
  - key: T0
    section: B0
    story: "the board tooling ledger is cut to the end state from the desk box's measurement"
    state: done
    done: 2026-09-21
    tasks: [TASK-0327]
    gate: "ledger reviewed"
  - key: T1
    section: B0
    story: "one command installs the flash and serial tools on Ubuntu, Fedora and Arch, the board reachable without sudo"
    state: done
    done: 2026-09-21
    tasks: [TASK-0327]
    gate: "make doctor green on all three families"
  - key: T2
    section: B0
    story: "just board-* recipes flash, watch and ack the board end to end"
    state: done
    done: 2026-09-21
    tasks: [TASK-0327]
    gate: "recipes run against the connected board"
  - key: T3
    section: B0
    story: "the board's marker ladder is read from the boot trace on its disk"
    state: done
    done: 2026-09-29
    tasks: [TASK-0327B]
    gate: "[PASS] board-headless"
  - key: T4
    section: B0
    story: "a driver bring-up starts with the measure-first playbook"
    state: done
    done: 2026-09-30
    tasks: []
    gate: ".claude/skills/driver-bringup/"
  - key: B1.0
    section: B1
    story: "the board is measured over adb and the hardware ledgers are cut to the end state"
    state: done
    done: 2026-09-22
    tasks: [TASK-0244, TASK-0245, TASK-0246, TASK-0260, TASK-0250, TASK-0251, TASK-0286]
    gate: "RFC-0098, ADR-0066 and ADR-0067 seeded"
  - key: B1.1
    section: B1
    story: "one bounded reader of the device tree, the one hardware truth"
    state: done
    done: 2026-09-22
    tasks: [TASK-0244]
    gate: "host tests against the checked-in trees"
  - key: B1.2
    section: B1
    story: "the kernel's platform comes from the device tree, position-independent"
    state: done
    done: 2026-09-22
    tasks: [TASK-0245]
    gate: "no platform literal outside the tree"
  - key: B1.3
    section: B1
    story: "one owner for clocks, resets, power domains and pinmux"
    state: done
    done: 2026-10-10
    tasks: [TASK-0245B]
    gate: "storage, display and USB through socd on the board"
  - key: B1.4
    section: B1
    story: "page-backed memory from the device tree's banks, the kernel in the high half"
    state: done
    done: 2026-09-24
    tasks: [TASK-0286, TASK-0284]
    gate: "frames per bank, contiguous DMA objects"
  - key: B1.5
    section: B1
    story: "the loader and the block owner read the eMMC through one SDHCI core"
    state: done
    done: 2026-10-10
    tasks: [TASK-0246, TASK-0246B]
    gate: "the GPT and the system volume mount over HS400ES"
  - key: B1.6
    section: B1
    story: "the board boots our chain from its eMMC into a living userspace"
    state: done
    done: 2026-10-10
    tasks: [TASK-0260, TASK-0260B]
    gate: "init: ready on the board"
  - key: B1.7
    section: B1
    story: "the desktop on the HDMI monitor at 1920x1080@60"
    state: done
    done: 2026-10-10
    tasks: [TASK-0250, TASK-0251]
    gate: "[PASS] board-visible, gpud: dc scanout ok"
  - key: U0
    section: B2
    story: "the USB topology and the HID reports are measured on the stock system"
    state: done
    done: 2026-10-04
    tasks: [TASK-0328, TASK-0253B]
    gate: "RFC-0099 seeded, HID parsers fixed to the measured reports"
  - key: U1
    section: B2
    story: "xhcid enumerates a keyboard and a mouse behind a hub on QEMU, by the interrupt alone"
    state: done
    done: 2026-10-05
    tasks: [TASK-0328]
    gate: "the usb lane in test-all"
  - key: U2
    section: B2
    story: "hidrawd takes USB HID and virtio-input as two sources of one contract"
    state: done
    done: 2026-10-05
    tasks: [TASK-0253B]
    gate: "usb-visible: SELFTEST: ui v2 input ok"
  - key: U3
    section: B2
    story: "the desk's keyboard and mouse drive the desktop over USB on the board"
    state: done
    done: 2026-10-06
    tasks: [TASK-0328, TASK-0253B, TASK-0245B]
    gate: "[PASS] board-visible, board-visual: typed"
  - key: B3.1
    section: B3
    story: "modal semantics: a modal holds focus, ESC and the backdrop dismiss it"
    state: done
    done: 2026-10-06
    tasks: [TASK-0074]
    gate: "board-visual: modal"
  - key: B3.2
    section: B3
    story: "the desktop tiles windows by default: halves, quarters, chords, the window menu"
    state: done
    done: 2026-10-07
    tasks: [TASK-0066]
    gate: "board-visual: tile"
  - key: B3.3
    section: B3
    story: "one clipboard authority"
    state: done
    done: 2026-10-08
    tasks: [TASK-0067]
    gate: "board-visual: clipboard"
  - key: B3.4
    section: B3
    story: "the clipboard in the shell search and on the keyboard, text editing in every field"
    state: done
    done: 2026-10-08
    tasks: [TASK-0067B]
    gate: "board-visual: clipboard"
  - key: B3.5
    section: B3
    story: "a screenshot: one readback authority, the frozen frame, PNG to Pictures"
    state: done
    done: 2026-10-09
    tasks: [TASK-0068]
    gate: "board-visual: screenshot"
  - key: L0
    section: L
    story: "memory, GPU and SMP measured against the stock system, the FAIL gate reads the kernel"
    state: done
    done: 2026-10-10
    tasks: []
    gate: "two measurement folders with verdicts"
  - key: L1
    section: L
    story: "the GPU driver and scanout contracts, host-tested"
    state: done
    done: 2026-10-12
    tasks: [TASK-0280]
    gate: "RFC-0101 Phase 0, 18 test_reject_*"
  - key: L2
    section: L
    story: "the scanout contract on the board, the console off the big kernel lock"
    state: done
    done: 2026-10-18
    tasks: [TASK-0216, TASK-0306]
    gate: "board front=0, bkl budget ok"
  - key: L3
    section: L
    story: "the GPU is online: firmware, MMU, power, the fence bridge"
    state: done
    done: 2026-10-13
    tasks: [TASK-0329]
    gate: "gpu fence ok, 131 us against the stock 640 us"
  - key: L4
    section: L
    story: "the GPU draws a textured quad into a plane of the ring"
    state: done
    done: 2026-10-13
    tasks: [TASK-0169B]
    gate: "gpu fill ok below the stock ns per pixel"
  - key: L5
    section: L
    story: "the whole desktop is composited on the GPU, the CPU composite deleted on the board"
    state: done
    done: 2026-10-19
    tasks: [TASK-0215]
    gate: "board-visual: gpu-desktop"
  - key: LU.0
    section: L
    story: "instant and smooth as numbers: the latency contract, the frame chain, no parked compositor"
    state: done
    done: 2026-10-22
    tasks: [TASK-0334, TASK-0338, TASK-0339, TASK-0341, TASK-0342, TASK-0344]
    gate: "ux lines judged on every lane, 0 NACKs, hover and scroll without an app raster"
  - key: LU.1
    section: L
    story: "the frame cost, one window path, frame pacing, motion on the compositor, the shell at once, the artifacts' design — the UX lane's rest, paused for layer 6"
    state: active
    tasks: [TASK-0335, TASK-0336, TASK-0337, TASK-0343, TASK-0345, TASK-0353, TASK-0354, TASK-0355]
    gate: "frame p99 <= 16.7 ms on the board; 0 of 32 frames dropped per shell transition; tap to frame <= 50 ms on the top bar and the launcher"
  - key: L6a
    section: L
    story: "what dies is freed: every kernel object dies with its last capability"
    state: done
    done: 2026-10-20
    tasks: [TASK-0333]
    gate: "rss loop ok (leaked=0) on the board"
  - key: L6b
    section: L
    story: "memory is committed on touch and shared with the GPU in one pool"
    state: done
    done: 2026-10-23
    tasks: [TASK-0333]
    gate: "vm commit on touch ok, gpud RSS 115.6 -> 49.4 MB"
  - key: L6c
    section: L
    story: "every process runs from a sealed image, code shared"
    state: done
    done: 2026-10-26
    tasks: [TASK-0333]
    gate: "exec image shared ok"
  - key: L6d
    section: L
    story: "an app's lifecycle: a hidden app is frozen, thawed when seen, one permission authority"
    state: done
    done: 2026-10-27
    tasks: [TASK-0333]
    gate: "ability lifecycle ok (fg->hidden->frozen->fg)"
  - key: L6e
    section: L
    story: "restore, accounting and pressure: memd the one memory authority"
    state: done
    done: 2026-10-27
    tasks: [TASK-0333]
    gate: "mem pressure kill ok, app restore ok"
  - key: L6f
    section: L
    story: "a frozen app is squeezed, swapped and, at a real low level, killed and recorded"
    state: done
    done: 2026-10-30
    tasks: [TASK-0333]
    gate: "lowmem lane: mem compress ok, swap out ok"
  - key: L6k
    section: L
    story: "no key lost, the caret at once, every shortcut in one registry"
    state: done
    done: 2026-10-30
    tasks: [TASK-0346]
    gate: "board cycles 78-79"
  - key: L6g
    section: L
    story: "apps draw where the GPU reads, any number of windows, and the compositor restarts under them"
    state: active
    tasks: [TASK-0347, TASK-0348]
    gate: "compose copies=0 and twelve windows on the board, windowd: restarted (ghosts=N)"
  - key: L8.1
    section: L
    story: "the memory model: an owner's footprint includes its surfaces, per-app limits, no fixed process tables"
    state: planned
    tasks: []
    gate: "a thirty-third app launches; a ninth app's surfaces are charged"
  - key: L8.2
    section: L
    story: "layer 8 on the GPU: direct scanout, apps rendering with the GPU, the passes a frame runs"
    state: active
    tasks: [TASK-0351]
    gate: "board: gpud: gpu gate scene ok, frame p99 <= 16.7 ms with glass, <= 4 ns per textured pixel on the median"
  - key: L7
    section: L
    story: "SMP, the dynamic kernel: n harts from the device tree, scheduler and IPC off the lock"
    state: planned
    tasks: [TASK-0330, TASK-0306]
    gate: "KSELFTEST: smp online ok harts=8 clusters=2"
  - key: L8.3
    section: L
    story: "placement by load, cluster-aware, and the compute-distribution seam"
    state: planned
    tasks: []
    gate: "per-hart utilisation gate on QEMU and the board"
  - key: N1
    section: NET
    story: "WiFi: the SDIO function and a FullMAC driver associate with the desk's network"
    state: planned
    tasks: [TASK-0248, TASK-0249]
    gate: "board wifid: assoc ok"
  - key: N2
    section: NET
    story: "Ethernet and a real lease: DHCP and DNS in netstackd"
    state: planned
    tasks: [TASK-0138]
    gate: "board: both links lease, DNS resolves; the network family's hold lifts"
  - key: N3
    section: NET
    story: "TLS on the device, entropy from the SoC's TRNG"
    state: planned
    tasks: [TASK-0193, TASK-0194]
    gate: "ingress TLS slot filled; no tolerated rng FAIL on the board"
  - key: N4
    section: NET
    story: "the clock loads from the internet, localized, the board's RTC as a second source"
    state: planned
    tasks: [TASK-0299]
    gate: "board sets real time and time zone"
  - key: N5
    section: NET
    story: "the proof lanes and the distributed family on a real network"
    state: planned
    tasks: [TASK-0331, TASK-0024, TASK-0030, TASK-0038, TASK-0040]
    gate: "just ci-network green, a two-device proof"
  - key: W1
    section: WEB
    story: "a Rust std target and a C/C++ toolchain port for isolated components"
    state: planned
    tasks: [TASK-0332]
    gate: "a cross-built std hello and a C++ test binary run on QEMU"
  - key: W2
    section: WEB
    story: "an isolated web engine renders a page into a surface, network only through the facade"
    state: planned
    tasks: [TASK-0111, TASK-0176, TASK-0177]
    gate: "QEMU and board: a local page in a surface"
  - key: W3
    section: WEB
    story: "the browser app: a DSL WebView, history, file chooser, persistence"
    state: planned
    tasks: [TASK-0113, TASK-0186, TASK-0187, TASK-0205, TASK-0206, TASK-0139]
    gate: "board-visual: browser"
---

# Milestone: hardware and network fast track

The OS boots on the reference board on the desk, shows a picture, takes USB input, finishes
Sub-80, and then builds the target pictures in layers: memory, a real GPU, real SMP, and after
them network and web. Started 2026-09-21; the steps above are the order. Progress is counted from
the steps (`nx ledger table hardware-network`), never written by hand.

The order inside block L follows the operator's decisions: layer 6 (memory and the app
lifecycle) was pulled before SMP on 2026-10-19; the UX lane paused for it on 2026-10-22 (LU.1);
the three-package plan of 2026-10-30 runs L6g, then the memory model (L8.1) and layer 8's GPU
work (L8.2) before SMP (L7).

## Decisions (end state)

**M — memory** (RFC-0100). A VMO is a page-backed object committed on demand; shared read-only
code from one sealed image; copy-on-write only where measured to pay; purgeable caches;
per-process accounting including GPU memory; four pressure levels as events; a compressor before
swap; swap on its own partition; `memd` the one termination authority, journaled.

**G — GPU** (RFC-0101). gpud is the thin driver-kit side (firmware under the provenance gate,
GPU MMU with explicit binding, submission over the driver-kit ring, explicit sync through kernel
fences); the userspace driver lives in `nexus-gfx` behind `GfxBackend`; compositor shaders are
compiled offline on the host. windowd and app-host never touch the GPU directly. Gaming is out
of scope.

**S — SMP** (RFC-0102 when written). The hart count comes from the device tree; every per-hart
structure is allocated from a boot slab sized by it; the CPU-set width (64 bits) is the one
compile-time ceiling. The big kernel lock is decomposed in a measured order (scheduler, IPC and
waitsets, address spaces, timers and fences). One kernel per machine; capabilities never travel;
distribution is pinched's job and only its seam is laid here.

**N — network** (RFC-0105). WiFi first (a FullMAC driver, firmware under the provenance gate,
`wifid`); Ethernet must not wait for it. Then DHCP and DNS, TLS on the device, the clock from
the internet, and the distributed family on a real network.

**W — web** (RFC-0103, RFC-0104). A `std` target plus a C/C++ runtime port; the engine runs as an
isolated component behind a Context/Frame/Navigation protocol, frames through per-app surfaces,
network only through the facade, no device capabilities. The old sanitizer renderer is deleted
from the plan.

## Hardware rules

1. **The device tree is the one hardware truth.** No MMIO address, IRQ number, clock or hart
   count lives in code outside the tree parser and its consumers. nxboot owns `/chosen/nexus,*`.
2. **host → QEMU → board** is the proof ladder of every package; the board lane reads the same
   proof manifest. A human-visible check is an operator-acked marker (`just board-ack`), never a
   claim in prose.
3. **Firmware blobs** only under `resources/firmware/<device>/` with a provenance file and a
   gate. Vendor kernel code is reference only.
4. **Names in files:** SoC and board identifiers, compatibles and USB ids are allowed; design
   comparisons stay generic.
5. **The board on the desk:** BPI-F3 (SpaceMiT K1, 8 RV64 harts in two clusters); details in
   `docs/board/`.

## Parked (no milestone yet)

Notifications, search, share, content and files apps, media and audio, accessibility, camera,
store, backup, power and sensors, session and accounts beyond layer 6d, the IME v2 rest,
renderer leftovers, the storage end-state ladder (0316–0320). These tasks stay `draft` and belong
to no step until a milestone takes them.
