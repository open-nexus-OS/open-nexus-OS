<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Icons

This document defines the **default icon contract** for Open Nexus OS UI.

See also: `docs/dev/ui/icon-guidelines.md`.

## Default icon set

- **Lucide** (ISC License)
  - Style: outline/line icons
  - Constraint: keep the set visually consistent by using a **small number of canonical sizes and stroke widths**.

## Canonical sizes & stroke widths

Treat these as **tokens** (do not pick random per-icon values):

- **Icon size**: 16 / 20 / 24 (default) / 32
- **Stroke width**:
  - 16 → 1.5
  - 20 → 1.75
  - 24 → 2.0 (default)
  - 32 → 2.0

Rationale: this keeps icons “line-sharp” but **not too thin**, and avoids visual drift across the UI.

## Rendering rules (determinism + crispness)

- Use the canonical **24×24 viewBox** for source assets where possible.
- Prefer rendering at **integer pixel sizes** and snapping translation/placement to whole pixels.
- Do not mix outline and filled variants in the same surface unless explicitly designed (filled icons should be a deliberate, separate style choice).
- If an icon needs optical adjustment, fix it at the asset/token level (not as one-off per usage).
