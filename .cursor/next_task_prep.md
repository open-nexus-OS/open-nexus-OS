# Next Task Prep (drift-check)

2026-06-12

## Last completed: TASK-0063 (UI v5b: Scene Graph GPU Pipeline + Virtual List + Theme Tokens + Virgl) — Done

## Next task: TASK-0064 (UI v6a: Window Management v1 — Chat-Window)

### Drift risk assessment

- **No drift detected.** TASK-0063 closed with RFC-0063 Complete. GPU-first pipeline is locked.
- TASK-0064 builds on the scene graph rendering authority established in TASK-0063.
- TASK-0064 was rescoped on 2026-06-12: Chat-Window als erste WM-Implementierung statt abstraktem WM-Layer.

### Pre-flight notes

- TASK-0064 requires: Chat-Button im SystemUiShell, Window/WindowManager structs, Title-Bar + X-Button, Drag-Mechanik
- Scene graph is sole rendering authority (RFC-0063 Complete)
- Input routing (hit-test) reuses existing pipeline from TASK-0056B/RFC-0051
- No kernel changes expected
- RFC-0064 (design seed) exists at `docs/rfcs/RFC-0064-ui-v6a-window-management-chat-window-contract.md`

### Blockers

- None known from TASK-0063 closure
- TASK-0063 deferred items (virgl TGSI compiler, GPU text rendering) do not block TASK-0064

### Scope boundaries (TASK-0064)

- **In**: Chat-Window (open/close/drag/focus), Chat-Button, Title-Bar + X, Z-Order
- **Out**: Resize, Multi-Window, Scene Transitions, IPC, Kernel changes
- **Follow-up**: TASK-0064B (Scene Transitions: Crossfade/Slide)
