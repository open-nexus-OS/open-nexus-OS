<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Token Reconciliation (design artifacts ↔ runtime)

> The design artifacts' token set is the one vocabulary of the system (RFC-0116, after the
> RFC-0070 amendment made the artifacts the design contract). This page maps each part of the
> set onto the places that carry it; the names and values themselves are the RFC's and the
> themes', not restated here. Companion: [`../../components/inventory.md`](../../components/inventory.md).

## One set, four places

| Part | The design artifacts | Themes (`resources/themes/*.nxtheme.toml`) | Typed tokens (`nexus-theme-tokens`) | DSL |
|---|---|---|---|---|
| colour roles | 31 roles, plus the wallpaper wash (readme) and the label halo (component stylesheet) | `[tokens]`, 35 keys per theme | `ColorToken` (`color_token.rs`) | `.fg` `.bg` `.borderColor` `.bgFade` `.tint` `.borderTint`, by name or from data |
| text styles | the type scale and the lock clock (display × 2): 13 styles | `[textStyle.*]` (base only) | `TextStyleToken`, `TextStyleSpec` (`design.rs`) | `.textStyle`, `.fontWeight` |
| spacing | the 4-pt grid and four semantic gaps | `[spacing]` (the names) | `SPACING_GRID`, `SpaceToken` | grid px or the names on every spacing modifier |
| radii | `sm` `md` `lg` `xl` `full`, concentric nesting | `[radius]` | `RadiusToken`, `concentric` | `.rounded`, `.corners` |
| control heights | `xs` … `xl` with their label styles | `[control]` | `ControlSize` | — (the kit widgets) |
| shadows | four elevations, the well, the lit edge | `[shadow.*]` (dark and high contrast override the colours) | `ShadowToken`, `ShadowSpec` | `.shadow(a, b?)`, plus `halo` |
| materials | the four backdrops and which surface blurs | `[material.*]` backdrops | `BackdropToken`, `recipe()` per `Material` | `.material(…)`, eight materials |
| motion | curves and durations | `[motion]` durations | `MotionDurationToken`, `MotionCurveToken` | `.animate` `.transition` `.effect` tokens |

The typed tokens are generated from the themes by `userspace/ui/theme-tokens/build.rs`, the
DSL's closed vocabularies live in `userspace/dsl/core/src/registry/vocab.rs` and resolve
through the typed tokens' own name tables in the runtime
(`userspace/dsl/runtime/src/registry/tokens.rs`). windowd reads the same typed tokens; there is
no second baked copy of a theme anywhere.

## Theme-varying vs. theme-invariant

The artifacts put colours under their dark override and the scales under the root only. The
themes mirror this:

- **Theme-varying** (per file): the colour roles and the shadow colours. `base` carries the
  light values, `light` overrides nothing, `dark` and `highcontrast` author every role that
  differs — the typed tokens' build fails if a variant leaves a theme-varying role to inherit
  the light value.
- **Theme-invariant** (authored once in `base`): text styles, spacing names, radii, control
  heights, shadow geometry, backdrops, motion durations, z-index.
- **High contrast** has no design contract: every theme-varying role maps onto black and white
  with strong edges, opaque surfaces and flat backdrops.

## Values

Every value is the artifacts' token file converted to hex: an rgba alpha becomes
`round(a × 255)`, an oklch colour goes through OKLab (the header of `base.nxtheme.toml`). A
change to a name is a change to RFC-0116; the set is versioned with the artifacts' token file.

## Old names

No alias of a removed name is kept. Every name the set retired is listed with its replacement
in `RETIRED_TOKENS` and `RETIRED_MODIFIERS` (`userspace/dsl/core/src/registry/vocab.rs`): the
checker quotes the replacement in its error, and the theme schema rejects the retired sections
(`userspace/ui/theme/src/schema.rs`).

## Verification

- `userspace/dsl/runtime/tests/token_vocabulary_lockstep.rs` — the checker's names, the
  runtime's maps and the typed tokens' names are one bijection; a retired name resolves
  nowhere.
- `userspace/ui/theme/tests/integration.rs` — every role resolves in every theme; dark and
  high contrast author every theme-varying role; the retired sections fail the load.
- `userspace/ui/theme-tokens/src/tests.rs` — the light and dark roles, the scales, the text
  styles and the control labels are the artifacts'; only the surfaces on the wallpaper blur.
- `userspace/dsl/core/tests/token_set.rs` — an old name, an off-grid spacing, a size outside
  the styles and a material inside a material are compile errors.
