# ADR-0035: SystemUI declarative shell configuration — manifests resolve the shell, windowd renders it

- Status: Accepted. Shell-A/B/D/E landed and boot-verified (desktop shell over virgl). Shell-C
  (boot systemui as a service) is DEFERRED — systemui is consumed as a library for now.
- Created: 2026-06-22
- Builds on: ADR-0028 (windowd present / visible-bootstrap), ADR-0009 (nexus-init bring-up),
  the unified `ShellWindow` compositor work (tasks #75–#80).
- Spec: `docs/dev/ui/foundations/layout/profiles.md` (the cross-device profile/shell model).
- Code: `source/services/systemui/` (resolver library), `source/services/windowd/` (consumer).

## Context

Which shell the device showed was **hardcoded**: windowd baked its own desktop chrome behind
compile-time constants (`SHELL_TOPBAR`, `SHELL_SIDEPANEL`, `USE_DESKTOP_SHELL`), systemui only
parsed a single desktop manifest (every validator literally rejected `id != "desktop"`), and
systemui was not part of the boot at all. There was no way to express "boot tablet", to switch
posture at runtime, or for a product/fork to ship a kiosk — the things
`docs/dev/ui/foundations/layout/profiles.md` calls for (a small set of well-known profiles + shells,
declared in TOML, extended by forks without patching core).

## Decision

The active shell is **declaratively configured in SystemUI and resolved into a plain value the
compositor consumes** — never hardcoded. Three manifest kinds, a registry, a resolver, and a
compositor-facing config:

```
  ui manifests (TOML)            systemui resolver (pure lib)         windowd (compositor)
  ┌───────────────────┐          ┌───────────────────────────┐       ┌────────────────────┐
  │ profiles/<id>      │  ──────▶ │ registry: catalog + lookup │       │ shell_config:      │
  │ shells/<id>        │          │ resolve_product(id)        │ ────▶ │   ShellConfig      │
  │ products/<id>      │          │   → {product,profile,shell}│       │ desktop_chrome,    │
  └───────────────────┘          │   → DeviceEnvironment       │       │ locked, …          │
                                  │ ShellConfig::from_resolved │       │ drives chrome +    │
                                  │ next_product_id / switch    │       │ launcher + lockdown │
                                  └───────────────────────────┘       └────────────────────┘
```

- **Manifests** (`source/services/systemui/manifests/`, `include_str!`-embedded):
  - `profiles/<id>/profile.toml` — device class (`id`, `default_shell`, `allowed_shells`, input
    flags, display defaults). Ships: `desktop`, `tablet`.
  - `shells/<id>/shell.toml` — shell posture (`id`, `kind`, `supported_profiles`, first-frame size,
    feature flags). Ships: `desktop`, `tablet`, `kiosk`.
  - `products/<id>/product.toml` — the **deployment config point**: chooses `profile` + `shell`
    (+ `theme`, `policy_preset`, `deployment`). Ships: `default` (desktop), `tablet`, `kiosk`
    (locked-down). A fork rebrands or locks down a device by adding ONE product manifest.
- **Registry** (`systemui::registry`): a compile-time catalog (`PROFILES`/`SHELLS`/`PRODUCTS` =
  `&[ManifestEntry]`). A profile/shell is "known" iff its manifest is registered — so forks add a
  manifest + one registry line, never a core enum arm. Validation is generic (value-domain checks +
  `profile.allowed_shells` ∩ `shell.supported_profiles`), with deterministic `ManifestNotFound` /
  `Incompatible*` rejects.
- **Resolution**: `resolve_product(id) -> ResolvedConfig{product, profile, shell, env}`;
  `resolve_default()` = the boot default. `DeviceEnvironment` is the stable `device.*` surface
  (`profile`, `shellMode`, `shellKind`, orientation, sizeClass, dpiClass, input).
- **Compositor-facing config**: `ShellConfig::from_resolved(&cfg)` flattens it to plain values
  (`shell_kind`, `desktop_chrome` = kind=="desktop", feature flags, `locked` = kiosk/locked-down,
  size). `shell_config_default()` is infallible (desktop fallback) so the compositor always boots.

windowd is a **pure consumer**: at `new()` it calls `systemui::shell_config_default()` and stores
`shell_config`. The old `SHELL_TOPBAR`/`SHELL_SIDEPANEL` constants are gone — the desktop chrome is
gated on `self.shell_config.desktop_chrome`. A boot marker records the resolved shell:
`windowd: shell config product=default profile=desktop shell=desktop kind=desktop chrome=true …`.

### Runtime switching + kiosk lockdown

`windowd.apply_shell_config(cfg)` swaps the active config at runtime: closes the dropdown, re-renders
the chrome surfaces, and damages the topbar + side-panel regions so both the virgl (rebuild-every-
present) and mmio (damage-driven) paths update. `cycle_shell()` advances to the next product via
`systemui::shell_config_next(current)`. A fixed **bottom-left corner hotspot** (always wallpaper,
reachable even in a chrome-less kiosk) is the current dev trigger.

A `locked` shell (kiosk kind / `locked-down` policy) enforces **kiosk lockdown**: `apply_shell_config`
closes any open chat/search, and `toggle_chat`/`open_search` refuse to open while locked — so a kiosk
is a clean locked surface, not just hidden chrome.

### Why systemui is a library, not yet a boot service (Shell-C deferred)

The intended end state is systemui as a running service that OWNS the shell decision and hands it to
windowd. We wired that (Cargo `[package.metadata.nexus-service]`, `declare_entry!`/`service_boot`,
init-lite + discover-services) — and it **stalled the boot**: `nexus-init`'s orchestrator spawns the
full service set, *then* emits `init: ready`, *then* runs the responder loop that distributes
routes/caps. systemui's spawn stalled that loop, so `init: ready` never fired, the responder never
ran, windowd never received its routes, and the **bootsplash→windowd handoff never happened**. Root
cause: `source/init/nexus-init/src/os_payload.rs` wires per-service receive slots
(`window_recv_slot`, `input_recv_slot`, …) for each *known* service; systemui had none.

Decision: systemui is marked `kind = "library"` (discover-services skips building/embedding its bin).
windowd consumes it **as a library** — the resolver runs in-process at windowd boot — so the UI is
already systemui-declared `desktop` over virgl WITHOUT booting the service. The `service_boot`/
`os_entry` code stays dormant for when the orchestrator wiring is extended.

## Consequences

- The shell is declarative and fork-extensible (add a TOML + a registry line; no core enum edits).
- desktop / tablet / kiosk are switchable at runtime today; only the **desktop** chrome has a real
  renderer — tablet/kiosk currently render as "chrome off" (+ kiosk lockdown). Real tablet/kiosk
  renderers are future work.
- windowd's chrome is config-driven, so the eventual systemui→windowd handoff is a value swap.
- Until Shell-C lands, the shell *decision* and the *render* both live on windowd's side (via the
  shared lib). The production trigger (settings / convertible event → systemui → windowd over IPC)
  replaces the dev corner-hotspot later.

## Verification

- Host: `cargo test -p systemui` (manifest resolve/switch/lockdown, 13 tests),
  `cargo test -p windowd`.
- riscv: `cargo check -p systemui --lib` / `-p windowd --features os-lite`,
  target `riscv64imac-unknown-none-elf`.
- Boot (the gate): `GPU_MODE=virgl just start` → desktop UI over virgl + the `windowd: shell config
  … kind=desktop` marker; bottom-left corner cycles desktop→tablet→kiosk→desktop.
- NEVER propose X11/Wayland/KDE/VNC for virgl issues (see ADR-0028 / the 2D-3D dual-wiring note).
