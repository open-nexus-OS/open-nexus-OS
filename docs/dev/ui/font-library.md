<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Curated Font Library (UI + Documents)

This page defines a **curated, open-source font set** for Open Nexus OS.

Scope:

- a small set that produces **good-looking apps** and **good-looking documents**
- avoids a “random font zoo”
- supports our initial language set: **English, German, Japanese, Chinese, Korean**

## Principles

- Prefer fonts with a clear, modern baseline and broad usage in real UIs/docs.
- Keep the set small and **pinned** (determinism).
- Prefer families that include multiple weights (regular/medium/semibold).
- Use explicit fallback chains for CJK; don’t rely on host OS fonts.

## Default system stacks (recommended)

### UI Sans (apps, system UI)

- **Primary**: Inter (SIL OFL 1.1)
- **Fallback (CJK)**:
  - Japanese: Noto Sans JP (SIL OFL 1.1)
  - Chinese (Simplified): Noto Sans SC (SIL OFL 1.1)
  - Chinese (Traditional): Noto Sans TC (SIL OFL 1.1)
  - Korean: Noto Sans KR (SIL OFL 1.1)
- **Fallback (everything else)**: Noto Sans (SIL OFL 1.1)

### UI Mono (code, logs, terminal-ish surfaces)

- **Primary**: IBM Plex Mono (SIL OFL 1.1)
- **Fallback**: Noto Sans Mono (SIL OFL 1.1) or Noto Sans (as last resort)

### Document Serif (reading + printing)

For “nice documents”, serif usually wins for long-form reading and print.

- **Primary (Latin)**: Source Serif 4 (SIL OFL 1.1)
- **Fallback (CJK)**:
  - Japanese: Noto Serif JP (SIL OFL 1.1)
  - Chinese (Simplified): Noto Serif SC (SIL OFL 1.1)
  - Chinese (Traditional): Noto Serif TC (SIL OFL 1.1)
  - Korean: Noto Serif KR (SIL OFL 1.1)

### Document Sans (clean “report” style)

- **Primary (Latin)**: Source Sans 3 (SIL OFL 1.1)
- **Fallback (CJK)**: same Noto Sans *CJK* chain as UI Sans

## Script / handwriting accents (optional, curated)

These are **accent fonts** for titles, greetings, “signature” UI, etc.
They are **not** suitable as default UI body fonts, and they typically do not cover all scripts.

### Latin (DE/EN) script

- **Great Vibes** (SIL OFL 1.1)
- **Allura** (SIL OFL 1.1)

### Japanese (handwriting)

- **Klee One** (SIL OFL 1.1)
- **Yomogi** (SIL OFL 1.1)

### Chinese (handwriting / calligraphic)

- **Ma Shan Zheng** (SIL OFL 1.1)
- **Zhi Mang Xing** (SIL OFL 1.1)

### Korean (handwriting)

- **Nanum Pen Script** (SIL OFL 1.1)
- **Gaegu** (SIL OFL 1.1)

Guidance:

- Keep these fonts behind a **bounded “accent font” selector** (don’t ship an unbounded font picker).
- For mixed-script text (DE/EN + CJK), prefer the document serif/sans stacks and use scripts only for short runs.

## Curated “style lanes” (optional, bounded)

If we want some stylistic choice without chaos, offer a few lanes:

- **Neutral Modern**: Inter + Source Sans 3 (documents in sans)
- **Modern Serif**: Inter UI + Source Serif 4 (documents in serif)
- **Classic Serif** (optional): Crimson Pro (Latin) + Noto Serif CJK

Rules:

- lanes are few (≈ 3–5)
- each lane must define both: UI Sans + Document Serif/Sans + Mono + CJK fallbacks

## Guidance for apps and documents

- Apps should default to the system UI stack unless they have a strong reason to opt out.
- Document editors should expose a small “font lane” selector, not an unbounded picker by default.
- For multilingual documents (DE/EN + CJK), ensure:
  - consistent line height policy across fallback fonts
  - deterministic font selection (script-aware), not host-dependent.
## Determinism and packaging

- Font versions must be pinned and shipped as part of the OS image/runtime.
- The fallback order is explicit and stable.
- Avoid features that introduce renderer variance (e.g., relying on external font fallback or missing glyph substitution differences).

## Related

- Typography contract: `docs/dev/ui/typography.md`
- Text rendering contracts: `docs/dev/ui/text.md`
