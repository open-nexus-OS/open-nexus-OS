# Platform-Class UI Performance Optimizations (QEMU Matrix)

**Date:** 2026-06-05  
**Scope:** Smooth 2D desktop interaction under QEMU (glass/blur/animation/windowing)  
**Status:** Design target for RFC-0059 Phase 6c-7 closure

---

## 1. Goal

Deliver **spuerbar fluessige Alltagsinteraktion** (window moving, hover, scroll, blur/glass transitions) with deterministic behavior and measurable frame pacing.

This document defines:
- optimization priorities,
- measurable targets,
- and what is realistically reachable in QEMU profiles (including `virgl` target mode).

---

## 2. Constraints

- CI profile must stay deterministic and reproducible (headless first).
- Visible smoothness claims require metric evidence, not marker-only green.
- Performance strategy must degrade gracefully under heavy blur/glass load.
- No dependence on one specific workstation setup for baseline correctness.

---

## 3. Optimization stack (ordered)

## 3.1 Frame pacing correctness first

1. Kernel timer capability (or bounded timeout fallback)
2. Present completion feedback (`present_id` correlation)
3. Bounded in-flight frames (target: max 2)

Without this, smoothness cannot be stable even with faster rendering.

## 3.2 Render pipeline closure

1. GPU-first single command stream per frame (Phase 6c)
2. Asynchronous fence + pipelining (Phase 6d)
3. Fixed-point hot loops for blur/blend/SDF (Phase 6e)
4. Golden + regression gates (Phase 7)

## 3.3 Load-shedding policy

Under overload:
- reduce blur quality first,
- then reduce effect radius/frequency,
- never allow unbounded queue growth or multi-second stalls.

---

## 4. QEMU profile matrix (required vs stretch)

| Profile | Purpose | Required target | Stretch target | Notes |
|---|---|---|---|---|
| `headless` | CI determinism | Functional pass, bounded memory/latency | n/a | Not a visual smoothness profile |
| `display-gpu` | GPU path correctness via markers | Stable submit->render->present ladder | n/a | UART/marker-oriented |
| `visible-bootstrap` | Local visible interaction (current default visible path) | p95 frame interval <= 20ms in medium scenes | p95 <= 16.7ms | Primary local UX profile |
| `visible-virgl` (target) | Host-accelerated visible mode | p95 <= 16.7ms in medium scenes, p99 <= 24ms | p95 <= 13ms | Requires virgl wiring + host GL support |

---

## 5. Heavy-scene targets (glass/blur)

| Scenario | Required target | Failure condition |
|---|---|---|
| Sidebar + hover transitions | no sustained sawtooth pacing | persistent oscillation / frame bunching |
| Medium blur panels | p95 <= 20ms | repeated >40ms spikes under normal interaction |
| Heavy blur burst | bounded degrade, no multi-second stalls | freeze-like stalls, unbounded in-flight growth |
| Input during animation | no starvation | visible input lag accumulation |

---

## 6. Virgl target mode (QEMU)

`virgl` is a **target profile** for host-accelerated rendering experiments and local smoothness gains.

Representative QEMU direction (example):

```bash
-display gtk,gl=on
-device virtio-gpu-pci,virgl=on,max_outputs=1,xres=1280,yres=800
```

Important:
- Current launcher advertises `QEMU_GPU_DEVICE` but does not yet wire it into runtime argument selection.
- `visible-virgl` should be treated as **opt-in local profile**, not CI baseline.

---

## 7. What is realistically reachable in QEMU

## 7.1 Near-term (after 6c + 6d)

- Clear improvement from current low-FPS state to responsive interaction in medium scenes.
- Stable pacing and reduced jitter when timer + present feedback are both active.

## 7.2 Mid-term (after 6e + 7)

- Consistent smoothness in common desktop interactions with bounded degradation under heavy effects.
- Regression-safe performance floor through p50/p95/p99 + memory gates.

## 7.3 Stretch (with virgl profile)

- Better local visible smoothness on compatible hosts.
- Still not guaranteed identical across all developer machines.

---

## 8. "Works for every dev" policy

### Compile and correctness
- Must work for all developers via existing build/test flows.

### Performance expectations
- Baseline guarantee: deterministic correctness + bounded behavior in CI/headless.
- Smoothness guarantee: validated on declared visible profiles (`visible-bootstrap`, optional `visible-virgl`) with published metrics.

Do not promise identical FPS across all hosts; promise bounded, measurable profile targets.

---

## 9. Phase-7 sign-off gates

All required:

1. Golden image suite passes (blur/shadow/rounded/text/cursor)
2. Pacing gates pass in `visible-bootstrap`
3. Input-latency-under-load gate passes
4. Long-run memory stability passes
5. QEMU profile matrix report is archived with artifacts
6. Kernel timer capability package is integrated and proven in active pacing path (planned 6-8d package scope)
7. Present completion feedback is integrated and correlated in active pacing path

Optional stretch:

- `visible-virgl` profile hits stretch targets on supported hosts

---

## 10. Explicit non-acceptance criteria

Do **not** claim platform-class smoothness when:

- `submit()` path is still validate-only/no-op,
- fence path is always pre-signaled,
- only markers are green but pacing metrics fail,
- performance improvements appear only on one ad-hoc machine.
