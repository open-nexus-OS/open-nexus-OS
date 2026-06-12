# Next Task Prep (drift-check)

2026-06-12

## Last completed: TASK-0063 (UI v5b: Scene Graph GPU Pipeline + Virtual List + Theme Tokens + Virgl) — Done

### Drift risk assessment

- **No drift detected.** TASK-0063 closed with RFC-0063 Complete. GPU-first pipeline is locked.
- TASK-0062 (Animation/NexusGfx) Done, RFC-0059 Complete.
- TASK-0059 (clip/scroll/effects) Done, RFC-0058 Complete.

## Next task: TASK-0064 (UI v6a: Window Management + Scene Transitions)

- File: `tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md`
- TASK-0063 follow-up-tasks list: [TASK-0275, TASK-0064]
- TASK-0064 builds on the scene graph rendering authority established in TASK-0063.

### Pre-flight notes

- TASK-0064 requires: window management, scene transitions, multi-window compositing
- Scene graph is sole rendering authority (RFC-0063 Complete); TASK-0064 builds on this invariant
- No kernel changes expected for window management layer

### Blockers

- None known from TASK-0063 closure
- TASK-0063 deferred items (virgl TGSI compiler, GPU text rendering, OS-build blockers) are documented in RFC-0063 delta analysis and do not block TASK-0064
- TASK-0062 Phase 7 (kernel timer capability) is deferred but does not block TASK-0064

### TASK-0063 open items (deferred, not blocking)
- Virgl GPU shader: TGSI/SPIR-V compiler integration
- GPU text rendering: Text primitive → CB commands
- OS build: CPU compositor modules restored but not deleted
- 120 Hz pacing proof: blocked on kernel timer capability
