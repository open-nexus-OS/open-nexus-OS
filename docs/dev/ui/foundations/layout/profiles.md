<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Profiles (Cross-Device)

We aim for one UI language across devices. Differences are in **affordances**, not in “desktop vs tablet visual identity”.

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

For QEMU and host fixtures, prefer a deterministic preset catalog over ad-hoc local resolutions.

TASK-0055 uses a deliberately tiny headless proof profile before the richer
dev-preset catalog exists: `profile=desktop`, `64x48`, `60Hz`. This is a
behavior proof for surface/layer/present sequencing only. It is not a visible
display preset and must not be used as a consumer display or scanout claim.

Recommended starter presets:

- `phone-portrait`
- `phone-landscape`
- `tablet-portrait`
- `tablet-landscape`
- `laptop`
- `laptop-pro`
- `convertible`

Each preset should define:

- profile
- shell ID
- orientation
- shell mode
- width / height
- refresh rate (`Hz`)
- scale / dpi class
- input flags

These presets are for bring-up, testing, and performance work. They are not the same thing as a production end-user
display picker.

## Recommended starter preset values

These are recommended **development presets** chosen to align with the HiDPI/2.0x “golden path” from
`docs/dev/ui/foundations/layout/display-scaling.md`.

| Preset | Profile | Shell mode | Orientation | Resolution | Hz | Scale | dpiClass | sizeClass | Input |
|---|---|---|---|---|---:|---:|---|---|---|
| `phone-portrait` | `phone` | `phone` | `portrait` | `1080×2340` | 60 | `2.0x` | `high` | `compact` | `touch` |
| `phone-landscape` | `phone` | `phone` | `landscape` | `2340×1080` | 60 | `2.0x` | `high` | `regular` | `touch` |
| `tablet-portrait` | `tablet` | `tablet` | `portrait` | `2048×2732` | 120 | `2.0x` | `high` | `regular` | `touch` |
| `tablet-landscape` | `tablet` | `tablet` | `landscape` | `2732×2048` | 120 | `2.0x` | `high` | `wide` | `touch`, `kbd` |
| `laptop` | `desktop` | `desktop` | `landscape` | `2560×1600` | 120 | `2.0x` | `high` | `wide` | `mouse`, `kbd`, `touch` |
| `laptop-pro` | `desktop` | `desktop` | `landscape` | `3024×1964` | 120 | `2.0x` | `high` | `wide` | `mouse`, `kbd`, `touch` |
| `convertible` | `convertible` | `desktop` (default) | `landscape` | `2560×1600` | 120 | `2.0x` | `high` | `wide` | `mouse`, `kbd`, `touch` |

Convertible note:

- `convertible` is a hardware/device profile.
- It should expose a **runtime shell toggle** between at least `desktop` and `tablet`.
- The toggle changes shell posture and affordances, not the underlying device identity.

## Upstream vs fork stance

Upstream should ship a small set of well-known profiles, shells, and dev presets.

Forks/product trees may:

- add new profile manifests
- add new shell manifests
- add product/deployment manifests
- point those manifests at their own DSL shell roots and profile overrides

The important contract is that the runtime model stays stable even when the concrete IDs differ between upstream and a
fork.
