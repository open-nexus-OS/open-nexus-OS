# Architecture Decision Records (ADRs)

ADRs are narrow decision records: one decision, one rationale, and its consequences — used when a
change is too granular or too cross-cutting to live inside a single RFC without causing churn.
RFCs (design contract seeds: interfaces, invariants, proof strategy) live in [`../rfcs/`](../rfcs/);
tasks (`tasks/TASK-*.md`) remain the execution truth. To add an ADR: copy [`template.md`](template.md),
take the next free number, fill it in, and add an index line below.

Note: **ADR-0019 was never filed; the number is retired to keep history stable.**

## Index

- [ADR-0001: Runtime Roles & Boundaries (Host + OS-lite)](0001-runtime-roles-and-boundaries.md) — Accepted
- [ADR-0002: Nexus Loader Architecture](0002-nexus-loader-architecture.md) — Accepted
- [ADR-0003: IPC Runtime Architecture](0003-ipc-runtime-architecture.md) — Accepted
- [ADR-0004: IDL Runtime Architecture](0004-idl-runtime-architecture.md) — Accepted
- [ADR-0005: DSoftBus-lite Architecture](0005-dsoftbus-architecture.md) — Accepted
- [ADR-0006: Device Identity Architecture](0006-device-identity-architecture.md) — Accepted
- [ADR-0007: Executable Payloads Architecture](0007-executable-payloads-architecture.md) — Accepted
- [ADR-0008: Clipboard Architecture](0008-clipboard-architecture.md) — Superseded by ADR-0070
- [ADR-0009: Bundle Manager Architecture](0009-bundle-manager-architecture.md) — Accepted (updated 2026-01-22 for manifest.nxb unification)
- [ADR-0010: Search Architecture](0010-search-architecture.md) — Accepted
- [ADR-0011: Settings Architecture](0011-settings-architecture.md) — Accepted
- [ADR-0012: Time Sync Architecture](0012-time-sync-architecture.md) — Accepted
- [ADR-0013: Notification Architecture](0013-notification-architecture.md) — Accepted
- [ADR-0014: Policy Architecture](0014-policy-architecture.md) — Accepted (original recipe-directory baseline superseded by Policy as Code v1 — RFC-0045 / TASK-0047)
- [ADR-0015: Resource Manager Architecture](0015-resource-manager-architecture.md) — Accepted
- [ADR-0016: Kernel Libraries Architecture](0016-kernel-libs-architecture.md) — Accepted
- [ADR-0017: Service Architecture](0017-service-architecture.md) — Accepted
- [ADR-0018: DriverKit ABI Versioning and Stability](0018-driverkit-abi-versioning-and-stability.md) — Proposed
- ADR-0019 — never filed; number retired
- [ADR-0020: Bundle Manifest Format (manifest.nxb with Cap'n Proto)](0020-manifest-format-capnproto.md) — Accepted
- [ADR-0021: Structured Data Formats (JSON vs Cap'n Proto)](0021-structured-data-formats-json-vs-capnp.md) — Accepted
- [ADR-0022: Modern image formats (WebP/AVIF) for wallpapers, screenshots, and thumbnail caches](0022-modern-image-formats-avif-webp.md) — Proposed
- [ADR-0023: StateFS Persistence Architecture](0023-statefs-persistence-architecture.md) — Accepted
- [ADR-0024: Updates A/B Packaging Architecture](0024-updates-ab-packaging-architecture.md) — Accepted
- [ADR-0025: QEMU Smoke Proof Gating (Networking + DSoftBus)](0025-qemu-smoke-proof-gating.md) — Accepted
- [ADR-0026: Network Address Profiles and Validation Semantics](0026-network-address-profiles-and-validation.md) — Accepted
- [ADR-0027: selftest-client Two-Axis Architecture](0027-selftest-client-two-axis-architecture.md) — Accepted
- [ADR-0028: windowd surface/present and visible bootstrap architecture](0028-windowd-surface-present-and-visible-bootstrap-architecture.md) — Accepted (amended 2026-06-02: GPU-only architecture per RFC-0059 Phase 6)
- [ADR-0029: input v1 host core architecture](0029-input-v1-host-core-architecture.md) — Accepted
- [ADR-0030: Layout Engine — Deterministic Pretext Philosophy](0030-layout-engine-deterministic-pretext.md) — Accepted
- [ADR-0031: Three-Layer Animation Architecture (Animation Engine + NexusGfx SDK + GPU Driver)](0031-three-layer-animation-architecture.md) — Accepted
- [ADR-0032: GPU Command Ring + Pipelined Present (gpud virtio-gpu)](0032-gpu-command-ring-and-pipelined-present.md) — Accepted
- [ADR-0033: Soft-real-time spine — waitset + timeline fence + DriverKit submit](0033-soft-real-time-spine.md) — Accepted (Phases 1–4; Phase 5 clean idle deferred)
- [ADR-0034: Reactive cursor architecture — hardware overlay first, decoupled software fallback](0034-reactive-cursor-architecture.md) — Accepted
- [ADR-0035: SystemUI declarative shell configuration — manifests resolve the shell, windowd renders it](0035-systemui-declarative-shell-configuration.md) — Accepted
- [ADR-0036: App lifecycle, process spawn, and app registry are three separate services — `abilitymgr`, `execd`, `bundlemgrd` (no new `appmgrd`)](0036-ability-lifecycle-vs-process-vs-registry-service-split.md) — Accepted
- [ADR-0037: Each app owns its surface VMO, lazily allocated when active and freed when closed](0037-per-app-surface-lazy-vmo-lifecycle.md) — Accepted
- [ADR-0038: One Rust SSOT for the windowd↔gpud display wire; Cap'n Proto stays the control plane](0038-display-wire-ssot-and-capnp-boundary.md) — Accepted
- [ADR-0039: Device-class driver architecture — bus-HAL + DriverKit + a thin device shim](0039-device-class-driver-architecture.md) — Accepted
- [ADR-0040: Unified logging policy, timing signposts, and the pure-observer proof model](0040-unified-logging-policy-and-observer-proof-model.md) — Accepted
- [ADR-0041: Never-black boot — held GPU splash → atomic desktop reveal](0041-never-black-boot-splash-atomic-desktop-reveal.md) — Accepted
- [ADR-0042: Cross-process surface transport — per-app VMO + present IPC + compositor blit (v1)](0042-cross-process-surface-transport.md) — Accepted
- [ADR-0043: User data lives in a dedicated filesystem service (`nxfs`); statefs stays the service-state KV](0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md) — Accepted
- [ADR-0044: Block-device layout for statefs + nxfs — two virtio-blk devices; GPT consolidation follow-up](0044-single-blk-device-gpt-partitions-block-layer.md) — Accepted (amended 2026-07-15 after TASK-0293 bring-up)
- [ADR-0045: pinched — system-internal compute broker with exchangeable backends](0045-pinched-compute-broker-and-backends.md) — Accepted
- [ADR-0046: nexus-workpool — deterministic same-AS parallel compute](0046-deterministic-parallel-compute-workpool.md) — Accepted
- [ADR-0047: nexus-inet — minimal interaction-net evaluator backend](0047-interaction-net-evaluator-backend.md) — Accepted
- [ADR-0048: standing userspace runtime-integrity detectors](0048-userspace-runtime-integrity-detectors.md) — Accepted
- [ADR-0049: BKL lock-classes + soft-realtime CPU placement (SMP=4 interactive)](0049-bkl-lockclass-and-softrt-cpu-placement.md) — Accepted
- [ADR-0050: Display-mode authority — compositor owns the mode, sourced from fw_cfg](0050-display-mode-authority.md) — Proposed
- [ADR-0051: Service wire frames are declared, not hand-coded — one codec SSOT crate `nexus-wire`](0051-declarative-wire-codec-nexus-wire.md) — Accepted
- [ADR-0052: Per-hart earliest-deadline timer arming + affinity-respecting steal park](0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md) — Accepted
- [ADR-0053: settingsd is the single authority for all settings; windowd never writes them](0053-settingsd-single-settings-authority.md) — Accepted
- [ADR-0054: Map failures keep their identity across the syscall ABI — no wildcard errno arms](0054-map-errors-keep-their-identity-across-the-abi.md) — Accepted
- [ADR-0055: `bootctld` is the single boot-state authority; `updated` becomes a client](0055-bootctld-single-boot-state-authority.md) — Accepted
- [ADR-0056: The kernel records WHY a task exited and exposes it through wait](0056-task-exit-reason-kernel-abi.md) — Accepted
- [ADR-0057: Restart re-provisions from declared topology only; clients re-resolve via samgrd](0057-service-restart-capability-re-resolve.md) — Accepted
- [ADR-0058: Boot Selection Block — derived projection with a field-level dual-actor write matrix](0058-boot-selection-block-dual-actor-discipline.md) — Accepted
- [ADR-0059: First-stage boot chain — `nxboot` loader position, self-relocation, measured-boot handoff ABI](0059-first-stage-boot-chain-nxboot-handoff.md) — Accepted
- [ADR-0060: Verified system volume — `bundlemgrd` is the volume verifier and bundle authority, `init` stays the sole spawner](0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md) — Accepted
- [ADR-0061: Inbound exposure is an intent through ONE gateway — no service binds a non-loopback address itself](0061-exposure-intent-instead-of-free-binds.md) — Accepted
- [ADR-0062: init synchronizes boot ONLY through `@ready` and one kernel stage fence — never through yields, resume order or time caps](0062-boot-stage-fence-and-readiness-barriers.md) — Accepted
- [ADR-0063: A proof lane declares its resource envelope, never dies silently, and CI is the authority for the full ladder](0063-proof-lane-resource-envelope-and-authority.md) — Accepted
- [ADR-0064: Request/reply is one trap per side with a direct handoff — the reply completes the caller's syscall, and no clock takes part](0064-request-reply-one-trap-per-side-direct-handoff.md) — Proposed (TASK-0054C P0 seed 2026-09-15)
- [ADR-0065: The app-host's frame-producing phase allocates from a generation arena, and the service allocator owns the scope](0065-app-host-frame-phase-allocates-from-a-generation-arena.md) — Proposed (TASK-0077C P0 seed 2026-09-20; measured: layout 226 KiB/call vs emit ~100 KiB, live drift 0, ceiling ~50-85 interactions)
- [ADR-0066: On hardware the booted chain is boot ROM → vendor SPL → OpenSBI → nxboot as the FIT payload — no vendor U-Boot in the booted system](0066-boot-chain-on-hardware-nxboot-as-fit-payload.md) — Accepted (2026-09-29)
- [ADR-0067: ONE block owner (`blkd`) whose backend the FDT selects — virtio-blk on QEMU, SDHCI on the board; the GPT plane above it never learns the difference](0067-one-block-owner-backend-selected-by-fdt.md) — Accepted (2026-10-10)
- [ADR-0068: Modal semantics live in the DSL runtime on the app-owned `.overlay()`; windowd keeps ONE routing verb](0068-modal-semantics-in-the-dsl-runtime-windowd-keeps-one-routing-verb.md) — Accepted
- [ADR-0069: Window tiling — geometry in windowd's WM, chords normalized by inputd, every UI element in the window kit](0069-tiling-chords-normalized-by-inputd-applied-by-windowd-ui-in-the-kit.md) — Accepted
- [ADR-0070: The clipboard is ONE authority (`clipboardd`) whose reads windowd's focus truth gates; its UI lives in the shell search and the keyboard](0070-clipboardd-one-authority-focus-gated-reads-ui-in-shell-and-keyboard.md) — Accepted
- [ADR-0071: Screen capture — the readback is gpud's, the freeze is windowd's, the files are screencapd's, the UI is the shell's](0071-screen-capture-readback-in-gpud-freeze-in-windowd-ui-in-the-shell.md) — Accepted
- [ADR-0072: GPU programs are written in WGSL, lowered into our own IR and compiled by our backends into a pipeline library per GPU — the driver holds none](0072-gpu-programs-wgsl-our-ir-our-backends-pipeline-library.md) — Accepted (2026-10-15)
- [ADR-0073: Every kernel object dies with its last capability, and a process's death is an event the kernel writes into an endpoint its parent bound](0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md) — Accepted (2026-10-20)
- [ADR-0075: The compositor never waits on a client — every delivery is non-blocking, what does not fit waits in a bounded outbox, and the kernel says when a client's queue has room](0075-the-compositor-never-waits-on-a-client.md) — Accepted (2026-10-21)
- [ADR-0076: The app scrolls and hovers; windowd composes the layers it declared — one scroll physics, one hover decision, no app raster for either](0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md) — Accepted (2026-10-21)
- [ADR-0077: Memory is committed on first touch — the user page fault fills a lazy page, and the kernel touches user memory only through a checked range](0077-memory-is-committed-on-first-touch-the-kernel-touches-user-memory-only-through-a-checked-range.md) — Accepted (2026-10-23, TASK-0333 6b-1)
- [ADR-0078: GPU memory is the one pool's — committed when the GPU is told its pages, its share a fraction of RAM read at boot](0078-gpu-memory-is-the-one-pools-committed-when-the-gpu-is-told-its-pages-its-share-a-fraction-of-ram.md) — Accepted (2026-10-23, TASK-0333 6b-2b)
- [ADR-0079: User-mode cache maintenance is the right of a non-coherent device's capability — the kernel enables it per process, on the first use, never for everyone](0079-user-cache-maintenance-is-the-right-of-a-non-coherent-devices-capability.md) — Accepted
- [ADR-0080: A process runs from a sealed image — its text and read-only data mapped from the image's own frames, its data copied, and there is no other exec path](0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md) — Accepted
- [ADR-0081: The boot image is one verified container — the kernel and a bootfs of sealed images; the loader places both, the kernel adopts the bootfs, init and every core program run from it, and no program embeds another](0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md) — Accepted
- [ADR-0082: A process is its address space — its parent orders it whole (freeze, thaw, kill), a leader's exit takes its threads, and a send that would park on a frozen receiver is refused](0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md) — Accepted
- [ADR-0083: The compositor states what is seen, the lifecycle authority decides, the parent orders — a hidden app is frozen after a grace and thawed when it is seen again](0083-the-compositor-states-what-is-seen-the-lifecycle-authority-decides-the-parent-orders.md) — Accepted
- [ADR-0084: One permission authority — policyd holds every app grant, the bundle manager publishes them from the verified volume, the launchers ask and never read a manifest](0084-one-permission-authority-policyd-holds-every-app-grant-published-by-the-bundle-manager-asked-by-the-launchers.md) — Accepted (2026-10-27, TASK-0333 6d-3)
- [ADR-0085: Memory pressure is an event — the pool tells ONE sink its level, the levels are watermarks the machine sets at boot, and a purgeable page goes first](0085-memory-pressure-is-an-event-the-pool-tells-one-sink-its-level-from-watermarks-the-machine-sets-and-a-purgeable-page-goes-first.md) — Proposed (2026-10-27, TASK-0333 6e-0; the purge is 6e-3)
- [ADR-0086: memd is the one memory authority — it hears the pool, learns the bands, orders a kill through the lifecycle authority — and a restorable app comes back where it was](0086-memd-is-the-one-memory-authority-it-hears-the-pool-learns-the-bands-orders-a-kill-through-the-lifecycle-authority-and-a-restorable-app-comes-back-where-it-was.md) — Proposed 2026-10-27 (TASK-0333 6e-1 built the restore half: `restorable = true` → `nexus.permission.STATE` → `svc.state`, the record `/state/app/@self/…` owned by kernel identity, written on the compositor's `hidden` sighting and at close, read once before the initial effects; memd itself is 6e-2)
- [ADR-0087: A frozen app's private memory is squeezed on its parent's order — the records live with the object, a touch brings the page back, and a frame goes only after every hart forgot it](0087-a-frozen-apps-private-memory-is-squeezed-on-its-parents-order-the-records-live-with-the-object-a-touch-brings-the-page-back.md) — Accepted (2026-10-28, TASK-0333 6f-1; both lanes + board cycle 75)
- [ADR-0088: The swap stage — memd pages sealed records to a `swap` partition, the kernel holds the key, a touch parks until the page is back](0088-the-swap-stage-memd-pages-sealed-records-to-a-swap-partition-the-kernel-holds-the-key-a-touch-parks-until-the-page-is-back.md) — Accepted (2026-10-28, TASK-0333 6f-2; both lanes + board cycle 76)
- [ADR-0089: Lane choreography is declared, the guest opens the input window, every console line is whole](0089-lane-choreography-is-declared-the-guest-opens-the-input-window-every-console-line-is-whole.md) — Accepted (2026-10-30, TASK-0333 6f-3; also: a device's answer is an event, the harness only observes, what a running system writes is the system's)
- [ADR-0090: The input source decides the route, windowd orders focus and keys, the compositor owns the caret](0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md) — Accepted (2026-10-30, TASK-0346; board cycles 78–79)
- [ADR-0091: No ambient authority over address spaces — a task maps and spawns only into its own space; the kernel bounds what a caller sizes](0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md) — Proposed (2026-10-31, TASK-0347)
- [ADR-0092: A surface is a buffer the app draws and the GPU reads — gpud allocates it for its owner, windowd composes by reference](0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md) — Proposed (2026-10-31, TASK-0348; supersedes ADR-0042 for app surfaces)
- [ADR-0093: Tables grow, bounded by the pool, never by a constant — reserving costs nothing, touching costs pages](0093-tables-grow-bounded-by-the-pool-never-by-a-constant-reserving-costs-nothing-touching-costs-pages.md)
- [ADR-0094: One kernel, three executors — an effect's mathematics is defined once in nexus-gfx, the shaders and the raster interpret it, the raster is the oracle, performance is a series](0094-one-kernel-three-executors-an-effect-s-mathematics-is-defined-once-in-nexus-gfx.md) — Accepted (2026-11-04, TASK-0351 P7/P8)
- [ADR-0095: An app renders with the GPU in a context of its own — at normal priority, bound from its own surfaces, charged to its footprint, granted by its manifest](0095-an-app-renders-with-the-gpu-in-a-context-of-its-own-at-normal-priority-charged-to-its-footprint.md) — Proposed (2026-11-04, TASK-0351 P4; the app-host GPU renderer is its consumer)
- [ADR-0096: A wake preempts — a task of a class above the running task's runs at the trap exit, and the soft-realtime chain is Interactive](0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md) — Proposed (2026-11-04, TASK-0351 P5; the board's pass cost was the tick, not the GPU)
