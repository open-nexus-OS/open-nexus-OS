# Pre-Flight — TASK-0057

## Before starting implementation

- [ ] Read RFC-0056 (contract seed)
- [ ] Read docs/dev/ui/foundations/visual/colors.md (semantic tokens)
- [ ] Read docs/dev/ui/foundations/visual/materials.md (.nxtheme.toml format)
- [ ] Read docs/dev/ui/foundations/visual/cursor-themes.md (BreezeX)
- [ ] Check freedesktop icon theme spec
- [ ] Verify TASK-0056C gates still green (just dep-gate, just diag-os)

## Quality gates (must pass before claiming phase done)

- [ ] just fmt-check
- [ ] just diag-os
- [ ] just dep-gate
- [ ] cargo test -p ui_v2b_host
- [ ] No unwrap/expect on untrusted input
- [ ] No kernel prints/logs/markers
