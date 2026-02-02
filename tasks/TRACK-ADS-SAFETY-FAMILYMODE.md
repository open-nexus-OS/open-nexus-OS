---
title: TRACK Ads Safety + Family Mode: safe monetization without “store bounce”, child-safe defaults, and policy-gated navigation
status: Draft
owner: @security @ui @runtime @platform
created: 2026-02-01
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy v1 (cap matrix + adapters + audit): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Policy v1.1 (runtime prompts + Privacy Dashboard): tasks/TASK-0168-policy-v1_1-os-runtime-prompts-privacy-dashboard-cli.md
  - System Delegation / System Surfaces (intents + user-mediated confirmation): tasks/TRACK-SYSTEM-DELEGATION.md
  - Share v2a (intentsd + shared_policy): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - System Delegation v1a (action intents + defaults): tasks/TASK-0126B-system-delegation-v1a-intent-actions-defaults-policy-host.md
  - Share v2b (SystemUI chooser): tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Share v2c (sender wiring incl. Browser/SystemUI): tasks/TASK-0128-share-v2c-app-senders-selftests-postflight-docs.md
  - App lifecycle + SystemUI navigation: tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md
  - Settings typed substrate: tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Settings UI + deeplinks: tasks/TASK-0226-settings-v2b-os-settings-ui-deeplinks-search-guides.md
  - Security & Privacy UI (caps editor + audit viewer): tasks/TASK-0137-ui-security-privacy-settings-permissions-audit.md
  - permsd/privacyd baseline: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - WebView (offline sandbox baseline): tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md
  - Browser app (offline + Open With): tasks/TASK-0113-ui-v19c-browser-app-openwith-downloads.md
  - WebView v1.2 (history/CSP): tasks/TASK-0205-webview-v1_2a-host-history-session-csp-cookies.md
  - WebView v1.2 (OS wiring): tasks/TASK-0206-webview-v1_2b-os-history-downloads-resume-csp-ui-recovery.md
  - App Store (track): tasks/TRACK-APP-STORE.md
  - Store v1a (host core): tasks/TASK-0180-store-v1a-host-storefeedd-storemgrd-ratings.md
  - Store v1b (OS Storefront UI + policy caps): tasks/TASK-0181-store-v1b-os-storefront-ui-selftests-policy-docs.md
  - Store v2.2a (licensed + parental controls core): tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md
  - Store v2.2b (purchase UX + parental UI): tasks/TASK-0222-store-v2_2b-os-purchase-flow-entitlements-guard.md
  - Search v2 OS execution router (deeplinks/open-with): tasks/TASK-0152-search-v2-ui-os-deeplinks-selftests-postflight-docs.md
---

## Goal (track-level)

Enable **safe monetization** for free apps (including games for kids) without allowing:

- “dark pattern” close buttons that move/appear late,
- automatic navigation to Store/Browser (“store bounce”),
- or accidental installs initiated by children.

## Motivation (why this track exists)

In a perfect world, this would not need to be system work. In reality, especially in kids’ games, ads are often rendered via
arbitrary HTML/JavaScript with deliberately confusing close buttons, delayed dismiss controls, and click targets that open
external content with **malicious intent** (store bounce, browser bounce, misleading install prompts).

Many devices are used by parents and grandparents, and the OS must provide:

- **a pleasant UX for adults** (free apps can still be funded),
- **safe defaults for kids** (no accidental Store/Browser navigation),
- **and predictable, auditable behavior** (policy-gated, deny-by-default).

This inevitably makes the “ads integration story” more constrained (and therefore harder) for everyone — but the goal is to
make it **safe and standardized**, not to ban monetization.

## Core stance (invariants)

- **Navigation is policy-gated**: opening Browser/Store (and install) must be mediated via OS routing/delegation and checked by `policyd` (deny-by-default).
- **Family Mode**: a system-level mode/profile can apply broad denials (e.g. games cannot open Store/Browser), while still allowing safe in-app ads.
- **No arbitrary JS ad runtime inside apps**: ad presentation must be **standardized** (OS-controlled UI primitives: close button, countdown affordance, click target).
- **User-mediated leaving the app**: “Learn more / Install” requires an explicit user action and a system confirmation surface (SystemUI).

## Store rule (distribution policy)

Direction: once we have a first-party App Store, we can make this a **store distribution rule**:

- Apps distributed via the Store must use the **safe ads surface** and must not embed arbitrary JS ad runtimes.
- Apps that do not comply can still be distributed via **sideloading**, but:
  - they get stricter default policies (no store/browser navigation by default),
  - and they receive an “untrusted source” disclosure.

Rationale: a **safe family environment** is more important than advertiser convenience. This will not solve the problem 100%,
but it materially reduces dark-pattern ad harm on the default path while keeping monetization possible.

## Family Mode (policy profile)

Direction: when Family Mode is enabled, categories like `games.*` should **not** be able to open Store/Browser or trigger
generic navigation intents. The toggle should be controllable at runtime (Settings).

Example policy sketch:

```toml
# recipes/policy/family-mode.toml

# When Family Mode is active:
[games.*]
deny = ["store.open", "browser.open", "intents.navigate"]

# Or per-app:
[games.kids_puzzle]
deny = ["store.open", "browser.open", "intents.navigate"]
```

Notes:
- The TOML above is **illustrative pseudocode** (we don’t yet freeze a “category policy” syntax here).
- This track does not introduce new capabilities; it documents how existing policy enforcement should be *composed* for safety.
- The actual “mode toggle” UX and persistence are owned by Settings tasks (`TASK-0225`/`TASK-0226`) and policy tasks (`TASK-0136`/`TASK-0168`).

## Store install: safety disclosure (also for free apps)

Direction:

- Even “free” installs must be gated by `store.install`.
- Before installation, SystemUI/Storefront should show a **clear disclosure**:
  - app name + publisher/signer
  - requested capabilities
  - warning flags (e.g. ads, in-app purchases)
  - if Family Mode is enabled: an additional “blocked / requires approval” gate

Owned by: `TASK-0181` (Storefront UI + policy caps), plus Store v2.2 parental controls (`TASK-0222`).

## Standardized Ads (no JavaScript-in-app)

Direction:

- Ads must be rendered through a **standardized surface** (OS-controlled primitives), not as arbitrary JS/HTML inside app UI.
- Close/dismiss behavior must be predictable:
  - consistent placement and visuals
  - no “invisible close”
  - no delayed close without an explicit UX affordance

Implementation would likely be a system service (e.g. `adsd`) and a small SDK wrapper for apps, but this track is only the
integration contract and the safety rationale.

## External ads (automated inventory) via a safe API

If external/automated ad networks are desired, they must integrate via a bounded service API (server-side fetch, no JS runtime in apps).

Illustrative API surface (directional):

```text
adsd:
  registerAdSlot(app_id, slot_id) -> ad_token
  fetchAd(ad_token, format) -> AdContent (static)
  reportImpression(ad_token)
  reportClick(ad_token) -> requires user confirmation
```

Flow (direction):

1. App registers an ad slot with `adsd`
2. `adsd` fetches ad assets server-side (no JavaScript in the app process)
3. `adsd` renders standardized ad UI (image/video + OS buttons)
4. Click-out requires explicit user action + SystemUI confirmation surface
5. In Family Mode, Store/Browser navigation is blocked by policy

## Tracking stance (realistic, but bounded)

Direction: ads will require analytics/tracking to be economically viable; banning all tracking is unrealistic.
Instead, tracking must be:

- **mediated** (through the ads surface/service, not arbitrary in-app JS),
- **capability/policy-gated** (deny-by-default; especially constrained in Family Mode),
- **audited** (click-outs, identifiers, and tracking events have a clear decision trail),
- **minimized by default** (coarse, non-identifying metrics preferred; stronger identifiers require explicit policy and UX).

Note: the concrete “tracking tiers” and identifiers are intentionally not specified here; they must be threat-modeled and aligned
with `TASK-0136`/`TASK-0168` privacy UX (prompts/dashboard/audit).

## Sideloaded apps: stricter defaults

Direction: sideloaded apps should have *at least* the same safety properties, and usually stricter defaults:

- less trust (no store review, unknown publisher),
- higher risk profile,
- consistency: no special bypass paths.

Example policy sketch:

```toml
[sideloaded.*]
deny = ["store.open", "browser.open", "intents.navigate"]
```

Optional: allow per-app override only via explicit Settings action (“Allow navigation”), with auditing.

## Naming consistency (capabilities + routing)

Direction: keep names consistent with existing tasks:

- Policy gating should primarily use the existing **intents capabilities** (`intents.send` / `intents.receive`) from `TASK-0136`,
  rather than introducing many new one-off caps.
- Store/Browser “open” should be treated as **system delegation / routing**, not as arbitrary app-to-app launching:
  - route via `intentsd`/SystemUI surfaces (see `TRACK-SYSTEM-DELEGATION` and `TASK-0126B`),
  - and enforce Family Mode + sideload defaults at the policy boundary.

The policy snippets in this doc use `store.open` / `browser.open` / `intents.navigate` as readable placeholders. When the
capability/action naming is finalized, replace them with the chosen canonical names (prefer: intent actions + `intents.send` gating).

## Store apps vs sideloaded apps (directional UX)

| Feature | Store apps | Sideloaded apps |
| --- | --- | --- |
| Ads | standardized surface allowed | standardized surface allowed |
| Store navigation | policy-gated | deny-by-default |
| Browser navigation | policy-gated | deny-by-default |
| Family Mode | blocked by policy | blocked by policy |
| Install warning | normal disclosure | extra disclosure (“untrusted source”) |

## Wiring map (who “owns” what)

- **Policy + capability model**: `TASK-0136`, `TASK-0168`
- **Delegation / routing surfaces**: `TRACK-SYSTEM-DELEGATION`, `TASK-0126` + `TASK-0126B`
- **SystemUI navigation + lifecycle**: `TASK-0065`
- **Store install/launch gates + parental controls**: `TRACK-APP-STORE`, `TASK-0181`, `TASK-0222` (and `TASK-0221` host core)
- **Browser/WebView constraints (no network by default; URL policy)**: `TASK-0111`, `TASK-0113`, `TASK-0206`
- **Settings storage + UX for toggles/modes**: `TASK-0225`, `TASK-0226`

## Non-goals (for this track doc)

- Defining a new ads SDK/service in this repo *right now* (that would be its own task later).
- Allowing generic web ads with arbitrary HTML/JS inside app processes.
