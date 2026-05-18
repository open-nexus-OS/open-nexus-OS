# Pre-Flight — TASK-0059

## Before starting implementation

- [x] Read RFC-0058 (contract seed)
- [x] Read RFC-0057 (layout engine contract)
- [x] Read docs/dev/ui/foundations/layout/layout-pipeline.md
- [x] Read docs/dev/ui/foundations/layout/scroll.md
- [x] Review windowd layout_panel.rs (filter-box integration point)
- [x] Review windowd proof_panel_spec.rs (FILTER_WORDS constant)
- [x] Verify TASK-0058 gates still green (just dep-gate, just diag-os)

## Quality gates (must pass before claiming phase done)

- [ ] just fmt-check
- [ ] just diag-os
- [ ] just dep-gate
- [ ] cargo test -p ui_v3b_host (Phase 4)
- [ ] RUN_UNTIL_MARKER=1 just test-os visible-bootstrap (Phase 5)
- [ ] No unwrap/expect on untrusted input
- [ ] No kernel prints/logs/markers
- [ ] Scroll damage math is allocation-free (stack-only scratch space)
- [ ] Layout computation not called in input hot-path
- [ ] filter_words("ap") returns 3 results deterministically
