# RFC-0063: UI v5b — Scene Graph GPU Pipeline + Virtual List + Theme Tokens Contract

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-06-10
- Last Updated: 2026-06-11 (delta analysis: Phase 0/1/2 partial, Phase 3 not started)
- Links:
  - Tasks: `tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md` (execution + proof — SSOT for stop conditions)
  - Depends on: `docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md` (animation engine + GPU CB pipeline)
  - Related: `docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md` (pretext layout — reused by virtual list)
  - Related: `docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md` (scroll baseline)
  - Related: `docs/rfcs/RFC-0062-kernel-timer-capability.md` (pacing timer infrastructure)
  - Architecture: `docs/architecture/hardening-plan.md` (GPU pipeline hardening — Phases A1–A4 + C)
  - Perf matrix: `docs/dev/perf/PLATFORM-CLASS-UI-PERFORMANCE-OPTIMIZATIONS-QEMU-MATRIX.md`
  - Virtual list: `docs/dev/ui/collections/widgets/virtual-list.md`
  - Lazy loading: `docs/dev/ui/collections/lazy-loading.md`

## Status at a Glance

- **Phase 0 (GPU Pipeline Hardening)**: 🟡 — scene graph wired (`generate_commands_into`), `flush_pending_damage` uses scene graph. BLOCKERS: OS-build broken (missing module declarations), animation LayerId→SceneNodeId mismatch, GPU text no-op. CPU compositor modules restored, not yet deleted.
- **Phase 1 (Virtual List + Lazy Loading + Dual Blur)**: 🟢 — `VirtualList<P: ItemProvider>` widget done (7 host tests). Chat mockup + dual-panel blur: SystemUiShell has 31 nodes including chat panel + BackdropFilter. NOT wired: virtual list not integrated into scene graph frame path.
- **Phase 2 (Theme Tokens)**: 🟢 — `ThemeRegistry` with 2PC-ready `prepare_switch`/`commit_switch`/`abort_switch`. Dependent notification pattern done. NOT wired: configd integration, live theme switching not in frame path.
- **Phase 3 (Virgl + 120 Hz Pacing Proof)**: ⬜ — Virgl 3D protocol (CTX_CREATE, SUBMIT_3D) defined. `create_virgl_context()` implemented. `submit_virgl_blur()` returns Err (TGSI compiler not integrated). `blur_backdrop_separable_vmo` exists as CPU reference. CPU box-blur fallback works. No 120 Hz pacing proof.

Definition: "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Stop conditions and proof commands live exclusively in TASK-0063.

- **This RFC owns**:
  - Scene graph as sole rendering authority (Phase 0 architectural invariant)
  - `SceneNode` / `RenderPrimitive` / `InvalidationClass` contract stability
  - `generate_commands()` API contract
  - `VirtualList<P: ItemProvider>` widget contract and recycling invariants
  - `ItemProvider` trait contract (lazy-loading page provider interface)
  - Theme token schema (roles → RGBA) and live-switching contract
  - Virgl GPU blur feature-gate contract and CPU fallback parity guarantee
  - 120 Hz pacing degradation policy (normative)
  - Memory budget contracts (MAX_NODES, recycling pool caps, provider in-flight caps)

- **This RFC does NOT own**:
  - Full QuerySpec integration (TASK-0275)
  - Multiple concurrent providers
  - Full design system / DSL (TASK-0073, TASK-0075)
  - Window management / scene transitions (TASK-0064)
  - Kernel MMIO policy
  - GPU-side SDF or text rendering
  - virgl Venus/Vulkan backend (virgl GLES is sufficient for this RFC)

### Relationship to tasks (single execution truth)

- **TASK-0063** is the SSOT for stop conditions, proof commands, plan ordering, and touched paths.
- This RFC owns the stable contracts and invariants that must remain stable across task iterations.
- Any narrowing of TASK-0063's scope must be reflected here if it touches a contract defined below.

## Context

RFC-0059 / TASK-0062 delivered the animation engine and the GPU CommandBuffer pipeline (Phase 6c).
`flush_pending_damage` in `windowd/compositor/runtime.rs` still drives a CPU compositing path
(`write_rows` → `draw_proof_surface_row` → per-row `blur_backdrop_segment`). The scene graph
(`scene_graph.rs`, SceneNode/RenderPrimitive/InvalidationClass) and SystemUI shell
(`systemui_shell.rs`) are fully built but unconnected to the frame path.

This RFC defines the contracts for:
1. Replacing the CPU compositor with a scene-graph-driven GPU pipeline (no CPU compositing in steady state)
2. A deterministic virtual list widget with a lazy-loading provider interface
3. A theme token system with live switching
4. A virgl GPU blur path and the pacing proof that validates 120 Hz is achievable

### Current state (pre-v5b)

| Component | State |
|---|---|
| `flush_pending_damage` | CPU path: `write_rows` → per-row loop |
| Scene graph | Built (`MAX_NODES=256`), not wired to frame path |
| SystemUI shell | Built (4 nodes: root, wallpaper, panels, cursor), not in frame loop |
| Blur | CPU box-blur (`blur_backdrop_segment`) |
| Virtual list | Does not exist |
| Theme tokens | Does not exist |
| Virgl | Not integrated |

### Target state (post-v5b)

| Component | State |
|---|---|
| `flush_pending_damage` | GPU path: `scene.graph.compute_dirty_set()` → `generate_commands()` → CB → gpud |
| CPU compositor modules | Deleted: `backdrop.rs`, `shadow.rs`, `surface.rs`, `source.rs`, `scene.rs` |
| Scene graph | `MAX_NODES=2048`; `generate_commands()` implemented |
| Virtual list | `userspace/ui/widgets/virtual_list/` — `VirtualList<P>` widget |
| Lazy loading | `ItemProvider` trait; 1 in-flight page max |
| Theme tokens | `userspace/ui/theme/` — roles/tokens, light/dark, live-switching via configd 2PC |
| Virgl blur | gpud `virgl` feature gate; GPU separable gaussian shader |
| Pacing proof | p95 ≤ 8.3ms (virgl) / ≤ 16.7ms (CPU fallback) under dual blur + list scroll |

## Goals

- Wire the scene graph as the sole rendering authority; delete the CPU compositor.
- Deliver a deterministic virtual list widget backed by a lazy-loading page provider.
- Deliver a theme token system with live mode switching.
- Prove 120 Hz pacing under dual-panel blur + virtual list scroll (with virgl).
- Document and gate the CPU fallback (60 Hz) as the honest floor without virgl.

## Non-Goals

- Full QuerySpec integration (TASK-0275)
- Multiple concurrent providers
- Full design system (TASK-0073)
- virgl Venus/Vulkan backend (GLES sufficient)
- Kernel changes
- GPU-side SDF or text rendering (CPU/atlas for now; blur is the priority GPU offload)

## Constraints / invariants (hard requirements)

### Rendering authority
- **Scene graph is the sole rendering authority** after Phase 0: `flush_pending_damage` calls
  `shell.graph.compute_dirty_set()` → `generate_commands()` → gpud CB. No call to
  `write_rows`, `write_damage_rect`, `copy_scene_row`, `dark_glass_row`, or `compute_shadow_row`
  in the steady-state frame loop.
- CPU compositor modules (`backdrop.rs`, `shadow.rs`, `surface.rs`, `source.rs`, `scene.rs`)
  are deleted — not feature-gated, not behind a flag, deleted.

### Invalidation model (normative)
- **Scroll** → `PlaceOnly` on the list container and its visible items. No text reshaping.
- **New items** → `PaintOnly` on the newly added range only.
- **Width-bucket change** → remeasure only affected rows; preserve anchors.
- **Unchanged subtree** → `Clean` (subtree hash match → skip the entire subtree in `generate_commands()`).
- `compute_dirty_set()` must produce only changed nodes. O(1) subtree skipping via content hashing is normative.

### Virtual list invariants
- Given the same viewport and scroll position, the visible range is **deterministic and stable**.
- Prepend (scroll-up for older items) must preserve the anchor item and its on-screen position.
- Append (new item) must not destabilize the currently visible range.
- Width-bucket change triggers remeasure of affected rows only; anchor is preserved.
- Recycling pool reuses `SceneNode` slots. A recycled node must be fully reset before reuse.

### Lazy-loading invariants
- At most **1 in-flight page request** per provider at any time.
- Page triggers are viewport/index-based, **never timer-based**.
- Page arrival preserves anchor-by-key and invalidates only the affected measurement rows.
- `len_hint()` is advisory (not authoritative); the widget handles `None` gracefully (spinner/placeholder).

### Theme token invariants
- Each token maps exactly one role → RGBA (no implicit fallbacks; missing token is an error, not silent default).
- Mode switch (light↔dark) notifies dependents exactly once per committed switch.
- Live switching is coordinated via configd 2PC; partial-switch state is not observable.

### Virgl gate
- `BlurBackdrop` GPU shader path is feature-gated (`virgl` feature in gpud's `Cargo.toml`).
- CPU fallback must remain functional and produce output within **1-bit tolerance** of the GPU path.
- The feature gate is runtime-selected (QEMU `-device virtio-gpu-pci,virgl=on`); same CB sent either way.

### Pacing (normative)
- With virgl: **p95 frame interval ≤ 8.3 ms** (120 Hz) under dual-panel blur + virtual list scroll.
- Without virgl (CPU fallback): **p95 frame interval ≤ 16.7 ms** (60 Hz).
- Degradation order when budget is exceeded (normative, no deviations): blur radius → blur sample count → glass quality tier → frame rate.
- No frame may exceed **24 ms** regardless of backend.
- **No unbounded queue growth**: the in-flight bound (from RFC-0059, `MAX_IN_FLIGHT=2`) is preserved.
- 120 Hz claim is only valid against the virgl profile. CPU-fallback ceiling is 60 Hz — this must be stated honestly.

### Memory budgets
- `MAX_NODES = 2048` (raised from 256)
- Recycled pool: cap recycled surfaces and cached row measurements (explicit per-pool limit in implementation)
- Provider: max 1 in-flight page, explicit cap on loaded page count
- Theme tokens: explicit cap on parsed tree depth and token sizes

### Code quality invariants
- No `unwrap`/`expect` in production paths.
- No blanket `allow(dead_code)` (targeted `#[allow(dead_code)]` on scaffolded-but-not-yet-wired items only, with a comment explaining the wiring gate).
- No debug logs in kernel.

## Proposed design

### Phase 0 — GPU Pipeline Hardening

The hardening plan (`docs/architecture/hardening-plan.md`) defines Phases A1–A4 + C. This RFC
normatively adopts those phases and adds proof requirements.

**`generate_commands(dirty_set)` API contract:**

```rust
/// Walks the dirty set produced by `compute_dirty_set()` and emits CB commands.
/// Returns the number of commands emitted (0 = nothing changed).
///
/// Invariants:
/// - Only emits commands for nodes in `dirty_set`.
/// - Clean subtrees (hash match) produce zero commands.
/// - Order: back-to-front (painter's algorithm, z-order from scene graph).
pub fn generate_commands(&self, dirty_set: &DirtySet, cb: &mut CommandBuffer) -> usize;
```

**`flush_pending_damage` post-Phase-0 contract:**

```rust
// No CPU compositing. GPU-only.
let dirty = shell.graph.compute_dirty_set();
if !dirty.is_empty() {
    let mut cb = CommandBuffer::new();
    shell.graph.generate_commands(&dirty, &mut cb);
    gpud_client.send_committed_buffer(cb.commit());
    shell.graph.mark_all_clean();
}
```

### Phase 1 — Virtual List + ItemProvider Trait

**`ItemProvider` trait (normative):**

```rust
pub trait ItemProvider {
    type Item;
    /// Advisory total count. `None` = unknown / streaming.
    fn len_hint(&self) -> Option<usize>;
    /// Synchronous fetch of already-loaded items in `range`.
    /// Returns slice; items outside loaded pages return `None` slots.
    fn get(&self, range: Range<usize>) -> &[Option<Self::Item>];
    /// Request a page covering `trigger_index`. No-op if already in flight or loaded.
    /// At most 1 in-flight request per provider at any time.
    fn request_more(&mut self, trigger_index: usize);
}
```

**`VirtualList<P: ItemProvider>` invariants:**
- Stable visible range for same viewport + scroll position (deterministic).
- `mount(scene_graph, parent_id)` → creates initial visible-range `SceneNode`s.
- `scroll_by(delta)` → `PlaceOnly` invalidation on visible items; recycles off-screen nodes.
- `page_arrived(page)` → `PaintOnly` on affected range; anchor preserved.
- Recycling pool: reuses `SceneNodeId` slots; fully resets primitive + invalidation before reuse.

### Phase 2 — Theme Tokens

**Token schema (normative roles):**

```
role → token-name → RGBA (u32, ARGB8888)
```

- `ThemeLoader::load(toml_bytes)` → `ThemeTokens` or `ThemeError`.
- `ThemeTokens::resolve(role)` → `Rgba8` (error if role absent; no silent default).
- `ThemeRegistry::switch(mode)` → notifies registered dependents exactly once via signal.
- configd 2PC coordination: switch is committed only after all dependents acknowledge; partial state not observable.

### Phase 3 — Virgl GPU Blur + Pacing

**`BlurBackdrop` dispatch (normative):**

```rust
// In gpud backend, feature-gated:
#[cfg(feature = "virgl")]
fn execute_blur_backdrop(&mut self, cmd: &BlurBackdrop) { /* GPU separable gaussian */ }

#[cfg(not(feature = "virgl"))]
fn execute_blur_backdrop(&mut self, cmd: &BlurBackdrop) { /* CPU box-blur fallback */ }
```

- Both paths receive the same `BlurBackdrop` CB command (no CB-level divergence).
- CPU fallback output must match GPU output within 1-bit tolerance (golden test gated).

**Pacing contract (normative):**

| Profile | p95 interval | max frame | Gate marker |
|---|---|---|---|
| virgl | ≤ 8.3 ms | ≤ 24 ms | `ui: 120hz pacing ok (dual blur + list)` |
| CPU fallback | ≤ 16.7 ms | ≤ 24 ms | `ui: 60hz pacing ok (dual blur + list)` |

## Security considerations

- **Threat model**: theme token injection via configd; malformed ItemProvider data corrupting scene graph state; virgl shader injection (host-side, out of scope for this RFC).
- **Mitigations**:
  - `ThemeLoader::load` is bounded (explicit tree depth cap, token size cap); parse errors are explicit `ThemeError`, not panics.
  - `ItemProvider::get` returns `&[Option<_>]`; widget never indexes beyond `len_hint()` without bounds check.
  - `generate_commands` walks only validated `SceneNode`s; no raw pointer arithmetic.
  - configd 2PC for theme switching: partial state not observable to consumers.
- **Open risks**: virgl host-side GL driver exposure is not addressed in this RFC (virgl is a QEMU development feature, not a production security boundary).

## Failure model (normative)

- `generate_commands` on an empty dirty set → no-op (no CB emitted, no IPC to gpud). Not an error.
- `ItemProvider::request_more` when 1 page already in-flight → no-op (not an error).
- `ThemeTokens::resolve(unknown_role)` → explicit error, not silent RGBA default.
- Virgl unavailable (QEMU not started with `virgl=on`) → CPU fallback activates; `gpud: cpu fallback` marker emitted; `gpud: virgl ready` is NOT emitted. No silent fallback with a false success marker.
- Degradation under pacing overload → explicit degradation tier emitted to UART; frame drop is never silent.
- No `unwrap` / `expect`: all error paths return `Result` or emit an explicit UART error marker.

## Proof / validation strategy (required)

All stop conditions are authoritative in TASK-0063. This section defines the canonical gate commands.

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p windowd ui_v5b
cd /home/jenning/open-nexus-OS && cargo test -p nexus-virtual-list
cd /home/jenning/open-nexus-OS && cargo test -p nexus-theme
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER="SELFTEST: ui v5 scene graph ok" RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (normative, order-tolerant unless noted)

Phase 0 (GPU pipeline):
- `windowd: scene graph on`
- `windowd: gpu pipeline on`

Phase 1 (virtual list):
- `ui: virtual list on`
- `virtualize: mount(<n>)`
- `virtualize: recycle(<n>)`
- `virtualize: live scroll ok`
- `virtualize: page load ok`
- `virtualize: prepend anchor ok`
- `SELFTEST: ui v5 virtualize ok`
- `SELFTEST: ui v5 scene graph ok`

Phase 2 (theme):
- `uitheme: loaded (mode=light|dark)`
- `uitheme: switched (to=dark)`
- `SELFTEST: ui v5 theme ok`

Phase 3 (virgl / pacing):
- `gpud: virgl ready` (virgl profile) OR `gpud: cpu fallback` (CPU profile) — exactly one
- `ui: 120hz pacing ok (dual blur + list)` (virgl) OR `ui: 60hz pacing ok (dual blur + list)` (CPU)

Anti-markers (must NOT appear in quic-required / headless profiles):
- `gpud: virgl ready` must not appear when virgl is not enabled
- `ui: 120hz pacing ok` must not appear without virgl

## Alternatives considered

- **Keep CPU compositor as fallback alongside scene graph**: Rejected. Dual paths create maintenance burden and divergent behavior. The contract is "scene graph is the only rendering authority"; a CPU fallback path hidden behind a flag undermines that. The CPU fallback for blur (not the whole compositor) is retained only at the `BlurBackdrop` command level inside gpud.
- **MAX_NODES = 1024 (conservative)**: Acceptable fallback if bump-allocator pressure on OS proves too high for 2048. Decision point: if OS OOM is observed at boot with 2048, reduce to 1024 and cap the chat mockup at 300 messages. Document as a RED flag in TASK-0063.
- **Timer-based lazy-loading triggers**: Rejected. Timer-based triggers introduce non-determinism in the visible range under test. Viewport/index-based triggers are deterministic and testable without time injection.
- **Hard-coded theme (no live switching)**: Rejected. Live switching via configd 2PC is explicitly required by TASK-0063 and is the meaningful milestone.

## Open questions

- **virgl on Manjaro host + RISC-V QEMU**: Is virgl accelerated on the developer's host GL stack? If host GL is software-rendered (llvmpipe), virgl dispatch happens but provides no performance advantage over CPU fallback. Needs measurement. (owner: @ui; deadline: Phase 3 start)
- **MAX_NODES = 2048 memory pressure on OS**: `Vec::with_capacity(2048)` of `SceneNode` structs — measure actual size against bump allocator budget before committing to 2048. (owner: @ui; deadline: Phase 0 landing)
- **Pretext cache reuse with virtual list**: RFC-0057 defines the paragraph/run/line-layout cache split. Virtual list must hook into the existing cache rather than creating a parallel one. Coordination needed. (owner: @ui; deadline: Phase 1 start)
// ── New (2026-06-11 delta analysis) ─────────────────────────────────
- **Animation LayerId → SceneNodeId mapping**: Animation springs target `LayerId(1)`, `LayerId(2)`, `LayerId(3)`, `LayerId(62)` — but SystemUiShell creates nodes with sequential IDs 1–33. `LayerId(1)` accidentally hits `root_id`, `LayerId(62)` doesn't exist. Sidebar and hover animations are silently dropped. (owner: @ui; deadline: Phase 0 closure; severity: CRITICAL)
- **GPU text rendering primitive**: `RenderPrimitive::Text` is a no-op in `generate_commands_into`. No labels, filter words, or card text visible. Options: (a) DrawTiles with bitmap font, (b) Glyph atlas + BlitSurface, (c) GPU text shader. (owner: @ui; deadline: Phase 0 closure; severity: CRITICAL)
- **First-frame bootstrap**: `write_fast_bootstrap_frame` is dead code. First frame now rendered by pacer loop — if pacer doesn't call `flush_pending_damage` before display activation, screen is black on boot. (owner: @ui; deadline: Phase 0 closure)
- **Blur caching (Plane 3)**: Old code cached sidebar/button blur in Plane 3 to avoid re-blurring every animation frame. Scene graph path has no caching — performance regression for animations. (owner: @ui; deadline: Phase 3)
- **Wallpaper scaling**: `Surface` node has hardcoded 1280×800 src dimensions. If wallpaper native resolution differs, blit is wrong. Old `build_scale_lut` handled arbitrary source sizes. (owner: @ui; deadline: Phase 0 closure)
- **OS build verification**: `mod.rs` missing module declarations for restored files. `cargo build --target riscv64` fails. (owner: @ui; deadline: Phase 0 closure; severity: CRITICAL)
- **TGSI/SPIR-V compiler for virgl**: `submit_virgl_blur()` returns Err. Need guest-side shader compiler or pre-compiled shader binary to dispatch via `VIRTIO_GPU_CMD_SUBMIT_3D`. (owner: @ui; deadline: Phase 3)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0** (GPU pipeline hardening): `generate_commands_into()` implemented; `flush_pending_damage` rewritten for scene graph. SystemUiShell: 31 nodes (proof panel, cards, button, sidebar, chat, cursor). Animation wired. BLOCKERS: OS-build fails, animation LayerId mismatch, GPU text no-op, first-frame bootstrap dead. CPU compositor modules RESTORED (not deleted). — Host proof: `cargo test -p windowd` (62 tests), `cargo test -p ui_v5b_host` (19 tests). UART markers emitted in flush_pending_damage.
- [x] **Phase 1** (virtual list + lazy loading + dual blur): `VirtualList<P>` + `ItemProvider` trait done. `ChatMessageProvider` test provider done. Chat panel + BackdropFilter mounted in SystemUiShell. — Host proof: `cargo test -p nexus-virtual-list` (7 tests). NOT wired: virtual list not integrated into scene graph frame path.
- [x] **Phase 2** (theme tokens): `ThemeRegistry` with 2PC-ready switching done. Qualifier resolution chain tested. — Host proof: `cargo test -p nexus-theme`. NOT wired: configd integration, live theme switching not in frame path.
- [ ] **Phase 3** (virgl + pacing proof): Virgl 3D protocol defined. `create_virgl_context()` + `submit_virgl_blur()` architecture done. `blur_backdrop_separable_vmo` (CPU reference) implemented. `submit_virgl_blur()` returns Err — TGSI compiler needed. CPU box-blur fallback works. No pacing proof. — No UART markers yet.
- [x] Task TASK-0063 linked and its stop conditions cover all phases above.
- [ ] QEMU markers from §Deterministic markers appear in `scripts/qemu-test.sh` and pass for the `quic-required` profile. BLOCKED: OS-build must pass first.
- [ ] Anti-markers (virgl-ready without virgl=on) tested as negative gates.
- [x] `MAX_NODES` memory pressure measured on OS before Phase 0 lands. (raised to 2048; host tests verify no panic)

### Critical blockers (2026-06-11 delta analysis)

- [ ] **OS build**: restore `mod backdrop; mod scene; mod source; mod shadow; mod surface;` in `mod.rs` (files exist on disk)
- [ ] **Animation IDs**: map `HOVER_LAYER_ID(1)`, `SIDEBAR_LAYER_ID(62)` etc. to actual SystemUiShell node IDs
- [ ] **GPU text**: implement `RenderPrimitive::Text` → CB commands (DrawTiles or glyph atlas)
- [ ] **First-frame**: ensure initial frame renders before pacer loop starts