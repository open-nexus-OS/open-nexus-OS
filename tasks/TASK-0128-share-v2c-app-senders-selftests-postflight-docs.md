---
title: TASK-0128 Share v2c: sender-side wiring (Files/Text/Images/Markdown/PDF/Browser/SystemUI) + OS selftests + postflight + docs
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Share v2a intentsd/policy: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Share v2b chooser/targets/grants: tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Files app share baseline: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Grants (content://): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
---

## Context

Share v2 is only useful if real apps can **send** and **receive** shares with strict limits and correct
grant handling.

This task wires common apps as senders and provides OS selftests + a postflight script that proves the
end-to-end flow in QEMU with deterministic markers.

## Goal

Deliver:

1. Sender-side wiring (apps):
   - Files: share selected `content://` URIs (with grants)
   - Text Editor / RichText: share selected text or document URI
   - Images: share current image as PNG bytes or URI
   - Markdown/PDF/Browser: share current document URI and/or selected text (where reasonable)
   - SystemUI: share shortcut can open chooser with last-copied clipboard payload (best-effort)
   - marker: `share: send (app=... mime=... uris=n text=len image=bytes)`
2. OS selftests (bounded):
   - wait for `intentsd: ready`
   - share text → chooser pick Clipboard target → `SELFTEST: share v2 text→clipboard ok`
   - share image → chooser pick Save target → `SELFTEST: share v2 image→save ok`
   - share text → chooser pick Notes target → `SELFTEST: share v2 text→notes ok`
   - grants enforcement: attempt URI share without grant (deny), then with grant (allow) → `SELFTEST: share v2 grants ok`
3. Postflight:
   - `tools/postflight-share-v2.sh` delegates to:
     - `cargo test -p share_v2_host`
     - bounded QEMU run + marker checks
4. Docs:
   - `docs/share/overview.md`: intents model, chooser behavior, results
   - `docs/share/providers.md`: clipboard/save/notes targets
   - `docs/share/policy.md`: budgets, scheme rules, sanitization, grant TTL rules
   - update testing docs with marker list

## Non-Goals

- Kernel changes.
- Remote share over DSoftBus (future).
- Full “Always use” UI preference management beyond `mimed` default (v1 uses mimed).

## Constraints / invariants (hard requirements)

- Deterministic markers, bounded timeouts.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required once deps exist

UART includes:

- `SELFTEST: share v2 text→clipboard ok`
- `SELFTEST: share v2 image→save ok`
- `SELFTEST: share v2 text→notes ok`
- `SELFTEST: share v2 grants ok`

## Touched paths (allowlist)

- `userspace/apps/files/`
- `userspace/apps/text*` / `userspace/apps/richtext`
- `userspace/apps/images/`
- `userspace/apps/markdown/` / `userspace/apps/pdf/` / `userspace/apps/browser/`
- `source/apps/selftest-client/`
- `tools/postflight-share-v2.sh`
- `docs/share/`

## Plan (small PRs)

1. sender wiring + markers
2. selftests + postflight
3. docs
