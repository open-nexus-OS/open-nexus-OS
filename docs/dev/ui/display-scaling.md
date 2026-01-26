<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display Scaling (Crisp Text & Icons)

This document defines the UI contract for **pixel density, scaling, and “crispness”**.

## What makes UIs look “crisp”

- **HiDPI**: higher pixel density reduces visible aliasing and makes text/icons look closer to print.
- **Integer scaling** (especially **2.0x**) helps because layout can map cleanly to device pixels.
- **Pixel snapping**: even on HiDPI, 1px/2px strokes and dividers look best when placed on stable pixel boundaries.

## Recommended “golden paths”

- **Golden path**: **2.0x** scaling (preferred)
- **Secondary path**: **1.0x** scaling (supported, but requires stricter snapping for thin lines)
- Avoid fractional scaling (1.25/1.5/1.75) as a default; it tends to introduce more “soft” edges and is harder to keep deterministic.

If the UI feels too large at 2.0x, prefer adjusting **density** (typography scale + spacing tokens) rather than switching to fractional scaling.

## Display class matrix (practical guidance)

Use this table as a rule of thumb for selecting the default scale and the preferred tuning knob.

| Display class (approx) | Typical devices | Examples (illustrative) | Recommended scale | Preferred tuning knob |
|---|---|---|---|---|
| **~90–140 PPI** (low/medium DPI) | TVs, low‑DPI external monitors, older laptops | 1920×1080 @ 24", 1366×768 @ 14" | **1.0x** | Increase **component sizes** (touch targets), keep strokes ≥ 2px where possible |
| **~140–180 PPI** (borderline HiDPI) | many modern laptops / external monitors | 1920×1080 @ 14", 2560×1440 @ 27" | Prefer **2.0x** if acceptable; otherwise **1.0x** | Adjust **density** first; avoid fractional default unless strongly needed |
| **~180–260 PPI** (HiDPI sweet spot) | premium laptops, tablets | **2256×1504 @ 13.5" (Framework)**, 2560×1600 @ ~13" | **2.0x** (golden path) | Adjust **density** (type/spacing tokens) for “more room” |
| **~260+ PPI** (very high DPI) | phones, some tablets | phone‑class panels | **2.0x** (or higher logical density) | Keep touch targets stable; tune typography scale by profile (phone/tablet) |
| **4K/5K/6K desktops** (varies by size) | high‑end desktops | 3840×2160 @ 27", 5120×2880 @ 27" | Prefer **2.0x** for quality | Density + compositor optimization (dirty rects/caching) to keep it fast |

Notes:

- **“Need more space”**: prefer smaller typography scale + tighter spacing tokens over fractional scaling.
- **Thin lines**: at 1.0x, avoid ultra‑thin strokes; ensure dividers and icon strokes snap cleanly.

## Framework Laptop “Original Display” (first target device)

Panel:

- 13.5", 3:2
- 2256×1504 @ 60Hz

Pixel density:

\[
\mathrm{PPI} \approx \frac{\sqrt{2256^2 + 1504^2}}{13.5} \approx 201
\]

Decision:

- **Default scaling**: **2.0x** (golden path)
- **Primary tuning knob**: **density** (typography + spacing), not fractional scaling

Why:

- ~201 PPI is clearly HiDPI-capable; 2.0x produces very stable, crisp text and outline icons.

## Future targets (desktop / tablet / phone / TV)

- **Tablet/phone**: generally HiDPI by default → 2.0x path is the “normal” case; ensure touch target sizing.
- **4K/5K/6K desktops**: aim for 2.0x as the primary quality mode; use compositor optimizations (dirty rects, caching) to keep it fast.
- **TV (10‑foot UI)**: crispness is dominated by **large sizes and contrast** more than DPI; treat it as a separate density profile with bigger typography and stronger focus states.

## Related

- Typography: `docs/dev/ui/typography.md`
- Icons: `docs/dev/ui/icons.md`
- Text rendering contracts: `docs/dev/ui/text.md`
