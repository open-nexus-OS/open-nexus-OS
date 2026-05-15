# Pre-Flight — TASK-0058

## Before starting implementation

- [x] Read RFC-0057 (contract seed)
- [x] Read docs/dev/ui/foundations/layout/layout-pipeline.md (pipeline contract)
- [x] Read docs/dev/ui/foundations/layout/text.md (text preparation contract)
- [x] Read docs/dev/ui/foundations/layout/wrapping.md (wrapping contract)
- [x] Read docs/dev/dsl/syntax.md (DSL naming conventions)
- [x] Read docs/dev/ui/foundations/visual/colors.md (semantic tokens)
- [x] Read docs/dev/ui/foundations/visual/typography.md (font contract)
- [x] Review windowd proof panel code (source/services/windowd/src/os_lite.rs)
- [x] Verify TASK-0057 gates still green (just dep-gate, just diag-os)
- [x] Confirm rustybuzz + fontdue only (no C libraries)

## Quality gates (must pass before claiming phase done)

- [ ] just fmt-check
- [ ] just diag-os
- [ ] just dep-gate
- [ ] cargo test -p nexus-layout (Phase 0+1)
- [ ] cargo test -p nexus-shape wrap (Phase 2)
- [ ] cargo test -p ui_v3a_host (Phase 3)
- [ ] RUN_UNTIL_MARKER=1 just test-os visible-bootstrap (Phase 4)
- [ ] No unwrap/expect on untrusted input
- [ ] No kernel prints/logs/markers
- [ ] No C dependencies in OS graph
- [ ] Fixed-point math only (no f32/f64 in layout)
- [ ] Proof panel pixel-identical regression gate maintained
