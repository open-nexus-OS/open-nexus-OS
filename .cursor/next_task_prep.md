# Next Task Prep (drift-check)

2026-06-10

## Next task: TASK-0063 (UI v5b: virtualized list + theme tokens)

### Drift risk assessment

- **No drift detected.** TASK-0062 closed with RFC-0059 locked. TASK-0063 is the next Fast-Lane task in sequence.
- RFC-0059 (animation/NexusGfx/gpud contract) is Complete; TASK-0063 builds on the GPU-first rendering pipeline established in 0062.
- TASK-0059 (clip/scroll/effects) is also Done with RFC-0058 Complete; any scroll/IME carry-in is reflected in the task contracts.

### Pre-flight notes

- TASK-0063 requires: input focus engine, virtual list recycling pool, theme token resolution, windowd/scenegraph integration
- No kernel changes expected
- RFC seed may be needed for theme token format (check task header for contract boundaries)

### Blockers

- None known
- TASK-0062 Phase 7 (golden tests, perf regression gates) is deferred but does not block 0063