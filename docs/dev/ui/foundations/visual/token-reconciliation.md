<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Token Reconciliation (design handoff ↔ runtime SSOT)

> STATUS: prep artifact for TASK-0073. Reconciles the **four divergent token
> representations** onto ONE runtime SSOT + ONE generated typed contract.
> Companion: [`../../components/inventory.md`](../../components/inventory.md), RFC-0070.

## The four representations that must collapse to one

| # | Representation | Location | Scope | Sourced from |
|---|---|---|---|---|
| 1 | **`.nxtheme.toml`** | `resources/themes/{base,dark,light,highcontrast}.nxtheme.toml` | ~15 tokens + 2 glass materials | authored (runtime SSOT candidate) |
| 2 | **typed `BaseTokens`** | `userspace/ui/theme-tokens/src/lib.rs` | 9 `ColorToken` + 7 `LengthToken` | **hardcoded — does NOT read #1** ⚠️ |
| 3 | **windowd `ThemeTokens`** | `windowd/src/theme.rs` + `assets::THEME_{DARK,LIGHT}` | 9 BGRA tokens | baked by build.rs **from #1** ✓ |
| 4 | **handoff CSS** | `docs/dev/design_handoff/reference/tokens/*.css` | ~30 colors + 4 glass levels + full type/spacing/radius/shadow/motion scales | the Apple-grade **target contract** |

**Target:** #1 (`.nxtheme.toml`) becomes THE single runtime SSOT, extended to carry the
full #4 contract; #2 is **generated from #1** (build.rs), no longer hardcoded; #3 folds into
the same generation path; #4 stays as reference/doc + the golden the runtime is checked against.

## Architectural split: theme-varying vs. theme-invariant

The handoff CSS puts **colors + glass** under `.dark` overrides (theme-varying) but **type/
spacing/radius/shadow/motion scales** under `:root` only (theme-invariant). Mirror this:

- **Theme-VARYING** (per `.nxtheme.toml`): colors, glass materials. Each theme file overrides.
- **Theme-INVARIANT** (ONE global table, authored once — e.g. `base.nxtheme.toml` `[scale.*]`
  or a dedicated `resources/themes/scale.nxtheme.toml`): typography sizes/weights/leading,
  spacing scale, radius scale, shadow scale, motion curves/durations, z-index. **Not** duplicated
  per theme. `highcontrast` overrides only colors + zeroes blur; it inherits the scales.

This is the clean rule that prevents the per-theme duplication the current 2-material setup
would otherwise force as we add the missing scales.

## Color token reconciliation (the semantics conflict)

⚠️ **`accent` means different things in the two sources — this is the load-bearing decision:**

| Concept | handoff CSS | nxtheme.toml | SSOT decision |
|---|---|---|---|
| interactive blue (buttons/toggle-on/selection) | `--color-info #3b82f6`, `--glass-toggle-on-bg rgba(59,130,246,.85)` | `accent` (#3b82f6 base / #2563eb light / #60a5fa dark) | **keep nxtheme `accent` = the interactive blue**; map handoff `info` → `accent` |
| neutral hover/selected wash | `--color-accent #e9ebef` (grey) | `surfaceAlt` (roughly) | **rename handoff-`accent` → `surfaceVariant`/`hover`** (don't collide with interactive accent) |
| primary brand ink | `--color-primary #030213` | (none) | add `primary`/`primaryFg` tokens |
| destructive | `--color-destructive #d4183d` | `danger #ef4444` | reconcile to one value (prefer handoff `#d4183d`); alias `danger`=`destructive` |
| success/warning/info | `#22c55e / #f59e0b / #3b82f6` | `success/warning` present, `info` missing | add `info`; align hexes to handoff |
| oklch neutrals | `oklch(...)` for bg/fg/muted/border | hex | **convert oklch→hex** at author time; document canonical hex per theme |

**Missing runtime color roles to add** (from handoff): `primary(Fg)`, `card(Fg)`, `popover(Fg)`,
`secondary(Fg)`, `info(Fg)`, `sidebar*` (7), `chart-1..5`, `island-bg`, `input-bg`, `switch-bg`,
plus the typed-token expansion (§ below). Existing `ColorToken` enum (9 roles) grows to cover them.

## Glass material reconciliation

Runtime has **2** materials (`glassLow`/`glassHigh`); handoff has **4 levels + overlay**:

| Handoff level | blur | dark bg | light bg | → SSOT material |
|---|---|---|---|---|
| `panel` | 40–64px (`blur-lg/xl`) | `rgba(255,255,255,.10)` | `.50` | `glass.panel` |
| `card` | 20px (`blur-md`) | `.08` | `.60` | `glass.card` |
| `subtle` | 12px | `.06` | `.70` | `glass.subtle` |
| `window` | dense gradient | `linear-gradient(...)` | `linear-gradient(...)` | `glass.window` (+ pane/bar/chip sub-tokens) |
| `overlay` | dense reading surface | `rgba(34,39,52,.80)` | `rgba(252,252,254,.82)` | `glass.overlay` (+ `scrim`) |

Plus glass sub-tokens to carry over: border per level, `shine` overlay, `hover/active` bg,
`divider`, `text-{primary,secondary,strong}`, `label-shadow`, `toggle-{on,off}-*`, `notif-dot`,
`icon-{bg,border}`. The runtime `blurRadiusDp` + `downsampleFactor` stay the **perf knobs**;
map the 4-tier px blur onto them (document the dp↔px + downsample choice per level).
`highcontrast` keeps `blur=0` (a11y) — no handoff contract, intentional.

## Scale reconciliation (net-new to runtime — currently hardcoded in windowd/nexus-style)

| Scale | Handoff values | Runtime today | Action |
|---|---|---|---|
| **typography** | xs 11 · sm 12 · base 14 · md 16 · lg 18 · xl 20 · 2xl 24 · 3xl 30 · 4xl 36; weights 400/500/600/700; leading tight/snug/normal/relaxed; tracking | windowd bakes 13px+16px A8 atlases only | add `[typography]`; drive atlas baking from it |
| **spacing** | 4px scale 0…96 (px, 0.5, 1…24) | ad-hoc consts | add `[spacing]` |
| **radius** | 6/8/10/14/16/24/full (base 10) | `LengthToken::Radius{S,M,L}` (8/16/24) | expand `LengthToken`; add `[radius]` |
| **shadow** | sm…2xl + icon/dock (5+4 steps) | `BoxShadow` ad-hoc | add `[shadow]` |
| **motion** | 5 curves (spring/spring-soft/spring-icon/smooth/glide) + 5 durations (0.10/0.16/0.28/0.40/0.50s) + reduced-motion | `animation` crate curves ad-hoc | add `[motion]`; **this is where motion lives — NOT a separate `ui/design` façade** |
| **z-index** | base/raised/overlay/modal/topbar/island | windowd stack consts | add `[zindex]` (advisory) |

## Typed contract expansion (`nexus-theme-tokens`)

- `ColorToken` (9 → ~30): add Primary, OnPrimary, Card, Popover, Secondary, OnSecondary,
  Info/Success/Warning/Danger(+Fg), Sidebar*, Chart1..5, IslandBg, InputBg, SwitchBg, plus the
  glass text roles. Keep it an **enum** (deterministic, DSL-mappable — a DSL `color.info` → `ColorToken::Info`).
- `LengthToken` (7 → full radius + spacing scale) — or split into `RadiusToken`/`SpacingToken`.
- Add `TypographyToken`, `MotionToken`, `ShadowToken`, `MaterialToken` (glass level) enums.
- **`BaseTokens` becomes generated** from `.nxtheme.toml` via build.rs (kill the hardcoded impl;
  it currently drifts silently from the toml — the core bug this reconciliation fixes).

## Canonical resolved color values (computed from handoff oklch, verified)

The handoff neutrals + charts are authored in **oklch**; the runtime SSOT is **hex**. Below are the
oklch→sRGB conversions (Björn Ottosson OKLab matrices; match the canonical shadcn/ui neutral+chart
set — e.g. `oklch(0.145 0 0)=#0a0a0a`, `oklch(0.269 0 0)=#262626`, `oklch(0.985 0 0)=#fafafa`).
**Design consequence:** the handoff palette is a **pure-neutral-grey** scale (Apple-like), not the
current slate-blue tint — adopting it shifts the OS neutrals to grey with **blue only as the
interactive accent**.

| oklch | hex | roles |
|---|---|---|
| `0.145 0 0` | `#0a0a0a` | dark bg/card/popover; light foreground/card-fg |
| `0.205 0 0` | `#171717` | dark sidebar; primary-fg; light sidebar-accent-fg |
| `0.269 0 0` | `#262626` | dark secondary/muted/accent/border/sidebar-accent |
| `0.439 0 0` | `#525252` | dark ring |
| `0.708 0 0` | `#a1a1a1` | dark muted-fg; light ring |
| `0.922 0 0` | `#e5e5e5` | light sidebar-border |
| `0.95 0.0058 264.53` | `#eceef2` | light secondary |
| `0.97 0 0` | `#f5f5f5` | light sidebar-accent |
| `0.985 0 0` | `#fafafa` | dark foreground/primary/*-fg; light sidebar/primary-fg |
| `0.68 0.196 25.5` | `#fa5a55` | dark destructive |
| charts light | `#f54900 #009689 #104e64 #ffb900 #fe9a00` | chart-1..5 |
| charts dark | `#1447e6 #00bc7d #fe9a00 #ad46ff #ff2056` | chart-1..5 |

Converter kept at `scratchpad/oklch.py` for regeneration; the values above are the authored SSOT.
Explicit hex handoff values carry over verbatim (primary `#030213`, destructive `#d4183d`,
success `#22c55e`, warning `#f59e0b`, info `#3b82f6`, toggle-on `#3b82f6@.85`, notif-dot `#ef4444`,
island `#000000`).

## Migration steps (W1, before any component work)

1. **[DONE — additive]** Author the reconciled **colors + 5 glass materials** into the
   `.nxtheme.toml` layers + `nexus-theme` (`resolve_material` chain added). Purely additive: every
   existing token name/value is unchanged, so windowd's bake stays byte-identical and all prior
   tests pass. New tokens (primary/card/popover/secondary/info/destructive/sidebar*/chart*/
   text-on-glass/toggle*) + the 5 handoff glass levels (`glassPanel/Card/Subtle/Window/Overlay`,
   replacing the ad-hoc `glassLow/High`) are pinned by goldens in `theme/tests/integration.rs`.
2. **[TODO — invariant scales]** Add `[typography]/[spacing]/[radius]/[shadow]/[motion]/[zindex]`
   sections (extend `KNOWN_SECTIONS` + validators + `Theme` struct). Theme-invariant, authored once.
3. **[TODO — palette shift, BOOT-GATED]** Retune the *existing* neutrals (`surface/fg/bg/accent/
   danger`) to the handoff pure-grey palette. This changes windowd's baked look → user boot-verifies
   before commit; updates the value-pinning tests (`dark bg`, `danger`, …). Deliberately deferred
   from step 1 so the vocabulary grows without a surprise appearance change.
4. **[TODO]** Regenerate `nexus-theme-tokens` typed enums + a generated `Tokens` impl **from the
   toml** (build.rs); delete hardcoded `BaseTokens`; point windowd `theme.rs`/`assets::THEME_*` at
   the same generation (one bake path). Golden-check generated values vs. the handoff `*.css`.
5. **[TODO]** Then build the glass primitive (RFC-0070 D4) that consumes the material tokens —
   the first real consumer of `resolve_material`.

## Verification

- Every `.nxtheme.toml` token has a handoff counterpart OR a documented "no contract" note
  (`highcontrast` colors, blur=0).
- Generated typed values == handoff `*.css` values (golden), oklch conversions documented.
- No component reads a raw color/length — only tokens (a11y-contrast lint enforces).
