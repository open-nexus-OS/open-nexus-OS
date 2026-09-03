<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Profiles (Cross-Device)

We aim for one UI language across devices. Differences are in **affordances**, not in “desktop vs tablet visual identity”.

> **Implementation status (2026-06-22).** The declarative model below is implemented: SystemUI parses
> profile/shell/**product** TOML manifests from a registry, resolves a product → profile + shell +
> `DeviceEnvironment`, and flattens it to a `ShellConfig` the compositor (windowd) consumes — the
> hardcoded `SHELL_TOPBAR`/`USE_DESKTOP_SHELL` constants are gone. Runtime shell switching
> (desktop↔tablet↔kiosk) and kiosk lockdown work. Shipped manifests: profiles `desktop`/`tablet`;
> shells `desktop`/`tablet`/`kiosk`; products `default`/`tablet`/`kiosk`. **Caveats:** only the
> `desktop` shell has a real renderer (tablet/kiosk currently render as “chrome off” + lockdown);
> SystemUI runs as a **library** resolver inside windowd, not yet as a booted service (see
> ADR-0035). Architecture + how it works: **`docs/adr/0035-systemui-declarative-shell-configuration.md`**.
> Code: `source/services/systemui/` (resolver + manifests), `source/services/windowd/` (consumer).
>
> **Update (2026-07-02, TASK-0065B):** the shell decision is now SESSION-driven: the login greeter
> (SystemUI greeter manifest — see `docs/dev/ui/shell/session.md`) runs first, and the logged-in
> user's `product` (from sessiond's user registry) selects the shell via `resolve_product` — the
> boot default is only the pre-login/fallback config.

## Profiles

- phone / tablet / desktop / tv / auto / foldable / convertible

Upstream should keep this starter set intentionally small. Forks and product trees may add their own profile IDs and
shell IDs without rewriting the core runtime model.

## Affordances

- desktop adds hover, precise pointer, and shortcuts
- tablet/phone emphasize touch targets

Rule of thumb:

- keep the same components and layout language,
- add pointer affordances (hover, right-click, shortcuts) without turning the UI into a legacy menu-bar app.

## Runtime/device environment

The DSL/SystemUI runtime should expose a small deterministic device environment rather than baking profile logic into
ad-hoc app code.

Recommended baseline:

- `device.profile`: validated profile ID
- `device.orientation`: `portrait | landscape`
- `device.shellMode`: validated shell ID / shell posture
- `device.sizeClass`: `compact | regular | wide`
- `device.dpiClass`: `low | normal | high`
- `device.input`: flags such as `touch | mouse | kbd | remote | rotary`

Posture:

- `device.profile` is the primary hardware / device-class axis.
- `device.orientation` refines the same profile (for example phone portrait vs phone landscape).
- `device.shellMode` selects the active shell posture when one device may legitimately host more than one shell
  (for example `convertible -> desktop|tablet`, or docked/TV-style environments later).
- Prefer one shared shell with responsive/base layout first, then profile-specific overrides only where needed.

Well-known upstream starter IDs:

- profile IDs: `phone`, `tablet`, `desktop`, `tv`, `auto`, `foldable`, `convertible`
- shell IDs: `phone`, `tablet`, `desktop`, `tv`, `auto`

## SystemUI posture

SystemUI should be profile-aware from the beginning:

- mount one canonical shell root,
- pass the stable device/profile environment into the DSL runtime,
- and let responsive layout + profile overrides decide the concrete shell shape.

When multiple shell postures are valid for the same device:

- keep the same app/runtime contracts,
- switch the shell via `device.shellMode`,
- and avoid treating shell switches as “boot a different OS product”.

Avoid:

- a permanently desktop-first shell that later gets “ported” to phone/tablet,
- or separate long-lived SystemUI products per profile.

## Declarative manifests

Profiles, shells, and product presets should be **declared**, not hardcoded across Rust/DSL branches.

Recommended authoring model:

- one TOML manifest per profile
- one TOML manifest per shell
- optional product/deployment TOML manifest that chooses profile + shell + theme/policy defaults
- strict schema validation with deterministic reject/fallback behavior

Recommended layout:

```text
ui/profiles/<profile-id>/profile.toml
ui/shells/<shell-id>/shell.toml
ui/products/<product-id>/product.toml
ui/platform/<profile-id>/
ui/shells/<shell-id>/
```

Why TOML:

- human-editable for forks and product teams
- already consistent with the repo's broader config direction
- strict enough for small schema-validated manifests
- easier to diff/review than a large monolithic config file

Do not rely on one giant `profiles.yml`-style file when multiple teams/products are expected. Small manifests are easier
to extend, merge, lint, and validate.

## Example manifests

Profile manifest:

```toml
id = "tablet"
label = "Tablet"
default_shell = "tablet"
allowed_shells = ["tablet", "desktop", "kiosk"]

[input]
touch = true
mouse = false
kbd = false
remote = false

[display_defaults]
orientation = "portrait"
dpi_class = "high"
size_class = "regular"
```

Shell manifest:

```toml
id = "companyNameShell"
kind = "kiosk"
dsl_root = "ui/shells/companyNameShell"
supported_profiles = ["tablet", "desktop", "convertible"]

[features]
launcher = false
multiwindow = false
quick_settings = true
settings_entry = false
```

Product/deployment manifest:

```toml
id = "acme-floor-terminal"
profile = "tablet"
shell = "companyNameShell"
deployment = "warehouse-floor"
theme = "acme-industrial"
policy_preset = "locked-down"
```

Fork workflow:

- a company can add a new profile manifest, a new shell manifest, or both
- the fork should not need to patch core SystemUI logic just to register a new profile or shell
- SystemUI/DSL should consume resolved manifest IDs and derived runtime values rather than scattered hardcoded enums
- unknown IDs or incompatible profile/shell pairings must fail deterministically with actionable diagnostics

## Dev-mode display/profile presets

> **Implementation status (2026-09-03, TASK-0055D).** The preset catalog is SHIPPED as
> declarative manifests: `source/services/systemui/manifests/presets/<id>/preset.toml`, one
> file per preset, registered in `systemui::PRESETS` and resolved by `systemui::resolve_preset`.
> Selection: `just start-preset <id>` (catalog: `just preset-list`, or `nx ui preset list`).
> Presets resolve INTO the existing authorities and never add one: profile/shell ids must be
> registered manifests with a valid pairing; the display mode becomes `QEMU_GPU_XRES/YRES`,
> i.e. the fw_cfg `display-mode` key the compositor already follows (RFC-0074 / ADR-0050);
> the emulated input set becomes `QEMU_PROOF_POINTER_SOURCE` for the launcher's injector.
> Host proof: `cargo test -p systemui` (catalog resolve, `test_reject_*` suite, convertible
> switch) + `cargo test -p nx --test ui_preset_cli` (process boundary, exit-3 reject).

For QEMU and host fixtures, prefer the deterministic preset catalog over ad-hoc local
resolutions (`QEMU_GPU_XRES=… just start` still works, but it bypasses validation).

Preset manifest shape (bounded TOML subset — the same parser as the profile manifests):

```toml
id = "tablet-portrait"
label = "Tablet (portrait)"
profile = "tablet"      # registered profile id (manifests/profiles/<id>)
shell = "tablet"        # registered shell id, allowed by the profile

[display]
width = 600
height = 800
hz = 120                # only pace-able rates (systemui::SUPPORTED_DISPLAY_HZ)
orientation = "portrait"
dpi_class = "high"

[input]
touch = true
mouse = false
kbd = false
remote = false
rotary = false
```

Validation (deterministic rejects, host-tested): unknown preset/profile/shell id
(`ManifestNotFound`), a shell the profile does not allow or that does not support the
profile (`UnsupportedShell`/`IncompatibleShell`), a mode outside
`[320, 1280×800]` or one contradicting its orientation (`InvalidDisplayMode`), a refresh
rate the pacer cannot deliver (`UnsupportedRefreshRate`), unknown orientation/dpi
vocabulary or a missing field (`InvalidManifest`/`MissingField`).

Resolved environment: `device.profile`/`device.shellMode`/`device.shellKind` from the
registered manifests, `device.orientation`/`device.dpiClass`/`device.input` from the preset,
`device.sizeClass` from the preset width via the runtime's own tiers (compact < 640 ≤ regular
< 1024 ≤ wide) — so the preset predicts what the DSL will see.

`convertible`: a hardware/device preset (tablet profile with `allowed_shells = [tablet,
desktop, kiosk]`). `systemui::switch_preset_shell` toggles the shell posture within the SAME
device identity (preset, profile, mode unchanged; environment re-resolved), reversibly. On
the device the same toggle is the Control Center Desktop/Tablet switch (settingsd
`ui.shell.mode`, the shell-mode authority).

These presets are for bring-up, testing, and performance work. They are not a production
end-user display picker.

## Shipped preset catalog (v1)

| Preset | Profile | Shell | Orientation | Mode | Hz | dpiClass | sizeClass | Pointer source |
|---|---|---|---|---|---:|---|---|---|
| `phone-portrait` | `tablet` | `tablet` | `portrait` | `480×800` | 120 | `high` | `compact` | `tablet` (touch) |
| `phone-landscape` | `tablet` | `tablet` | `landscape` | `800×480` | 120 | `high` | `regular` | `tablet` (touch) |
| `tablet-portrait` | `tablet` | `tablet` | `portrait` | `600×800` | 120 | `high` | `compact` | `tablet` (touch) |
| `tablet-landscape` | `tablet` | `tablet` | `landscape` | `1280×800` | 120 | `normal` | `wide` | `tablet` (touch) — **baseline** |
| `laptop` | `desktop` | `desktop` | `landscape` | `1280×800` | 120 | `normal` | `wide` | `mouse` (+kbd) |
| `laptop-pro` | `desktop` | `desktop` | `landscape` | `1280×800` | 120 | `high` | `wide` | `mixed` (touch+mouse+kbd) |
| `convertible` | `tablet` | `tablet` (toggle → `desktop`) | `landscape` | `1280×800` | 120 | `normal` | `wide` | `mixed` |

`tablet-landscape` is the canonical baseline every default QEMU proof boots
(`systemui::BASELINE_PRESET_ID`); the marker ladder asserts its literal
`windowd: ready (w=1280, h=800, hz=120)`. Other presets print the same marker with their real
mode (`windowd: ready (w=600, h=800, hz=120)`), so a preset boot never claims the baseline.

Honest limits of v1 (recorded in the TASK-0055D ledger, follow-up TASK-0322):

- **Mode ceiling 1280×800**: the compositor's shared atlas/VMO layout is sized to it and gpud
  clamps any larger fw_cfg mode, so larger presets are rejected instead of silently shrunk.
  The HiDPI 2.0× "golden path" resolutions from `display-scaling.md` stay a target, not a
  preset value.
- **Hz = 120 only**: windowd paces every mode at 120 Hz (`PACER_INTERVAL_NS`); a preset naming
  60 Hz would promise pacing the guest cannot deliver and is rejected.
- **Guest profile/shell follow the product/settings, not the preset**: the boot product
  (tablet) and the settingsd `ui.shell.mode` key select the shell on the device; the preset's
  `profile`/`shell` are validated + exported (`NEXUS_UI_PRESET_*`) but not yet ingested by the
  guest. Guest ingestion (fw_cfg key → settingsd default overlay → `systemui: profile …`
  marker + `SELFTEST: ui preset boot ok`) is TASK-0322.
- `phone`/`laptop` are not guest profiles yet: phone presets use the `tablet` profile (touch
  posture, compact width), laptop presets the `desktop` profile.

## Upstream vs fork stance

Upstream should ship a small set of well-known profiles, shells, and dev presets.

Forks/product trees may:

- add new profile manifests
- add new shell manifests
- add product/deployment manifests
- point those manifests at their own DSL shell roots and profile overrides

The important contract is that the runtime model stays stable even when the concrete IDs differ between upstream and a
fork.
