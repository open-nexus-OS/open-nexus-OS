# Kernel Timer Capability + Sauberer VSync-Pfad

**Date:** 2026-06-05  
**Scope:** Design-Ready Architektur fuer echte Timer-Capabilities und reaktiven VSync in `windowd`  
**Status:** Analyse/Design (kein aktiver Implementierungs-Task)  
**Zielniveau:** "10/10" Spezifikation fuer RFC-Seed + Umsetzungs-Task

---

## 1) Executive Summary

Wir haben heute bereits eine wichtige Grundlage: Der Kernel kann bei IPC mit Deadline echte Hardware-Wakeups programmieren (`set_wakeup(deadline_ns)`), also nicht nur Userspace-Polling.

Was fehlt, ist **ein first-class Timer-Objekt als Capability** (`timer_create/timer_set`) und damit ein sauberer Event-Pfad fuer VSync ohne `NonBlocking + yield_()` Workaround in `windowd`.

**Empfohlene Zielarchitektur:**

1. Timer als Capability im Kernel (OneShot + Periodic, absolute monotonic deadlines)  
2. Timer-Expiry wird als deterministisches IPC-Event zugestellt  
3. `windowd` laeuft blocking/reactive und tickt Animationen nur auf Input- oder VSync-Event  
4. Present-Completion Feedback (`gpud` -> `windowd`) taktet die finale Frame-Pacing-Schicht  
5. Deadline-basierter Timeout-Pfad bleibt als robuster Fallback

**Wichtig:** Kernel-Timer allein ist nur die halbe Optimierung. Fuer spuerbar "smooth" bei Glas/Blur braucht es zusaetzlich Present-Feedback und ein belastbares Pacing-Modell.

---

## 2) Ist-Zustand (korrigiert, codebasiert)

### 2.1 Was bereits existiert

| Schicht | Ist | Bedeutung |
|---|---|---|
| Hardware/HAL | `Timer::set_wakeup(deadline_ns)` via SBI `set_timer` | Monotonic wakeups vorhanden |
| Kernel IPC v1/v2 | `deadline_ns` in send/recv Syscalls | Blockierte Tasks koennen mit Timeout geweckt werden |
| Scheduler | Expired block reasons werden aktiv gewaked | Kein "ewig blockieren" bei Deadline-Expiry |
| Services | `Wait::Timeout(Duration)` wird breit genutzt | Bereits etablierter API-Pfad fuer zeitgesteuerte Waits |

### 2.2 Was fehlt

| Luecke | Impact |
|---|---|
| Keine `Timer` Capability im Cap-System | Kein eigener Timer-Lifecycle, keine explizite Timer-Ownership |
| Kein `timer_create/timer_set` ABI | Kein standardisierter Timer-Contract fuer Dienste |
| Kein dedizierter Timer-Event-Frame | VSync/periodische Events muessen indirekt geloest werden |
| `windowd` nutzt `Wait::NonBlocking + yield_()` | Unsauberer Event-Loop, unnötige Scheduler-Aktivitaet |

### 2.3 Konkrete Ursache fuer den unsauberen VSync-Pfad

`windowd` ist explizit als Workaround auf Polling-loop dokumentiert und implementiert.  
Ohne Timer-Capability bleibt die Animationstaktung in der Service-Loop statt in einem sauberen Timer-Event-Kanal.

---

## 3) Target Behavior (Behavior-First)

### 3.1 Soll-Verhalten

1. `windowd` schlaeft blockierend und wird nur durch **Input** oder **VSync-Timer-Event** geweckt.  
2. Animationen werden mit stabiler Periodik (z. B. 60Hz/120Hz) ohne akkumulierte Drift getickt.  
3. Bei Last/Backpressure bleibt Verhalten deterministisch (coalesced Ticks statt Event-Sturm).

### 3.2 Haupt-Breakpoint

Wenn Timer-Events ausfallen, doppelt zugestellt werden oder driften, dann ist der VSync-Pfad unehrlich/unzuverlaessig.  
Genau dort muessen die Kern-Proofs ansetzen.

### 3.3 No-Half-Measures Kriterium

Das Vorhaben gilt **nicht** als erfolgreich, wenn nur ein Kernel-Timer existiert, aber:

- Frame-Pacing weiterhin ohne Present-Completion laeuft,
- Queue-Backpressure Frames unkontrolliert verschiebt,
- Glass/Blur-Peaks weiterhin zu sichtbarem Stottern fuehren.

Erfolg heisst: spuerbar weniger Ruckeln unter Last, nicht nur "architektonisch sauberer".

---

## 4) Zielarchitektur

### 4.1 High-Level Datenfluss

`TimerCap` -> Kernel Timer Queue -> Timer IRQ -> IPC Event an Endpoint -> `windowd` recv -> `tick(now)` -> `flush_pending_damage()`

### 4.2 Komponenten

1. **Timer Capability (Kernel Object)**  
   - Eigentum via CapTable  
   - gebunden an Notify-Endpoint  
   - Zustand: disarmed / armed(oneshot|periodic)

2. **Timer Manager (Kernel, pro Hart/CPU)**  
   - min-heap oder geordnete Struktur nach `deadline_ns`  
   - rearmt den naechsten Wakeup auf fruehestes Deadline

3. **Timer Event Delivery**  
   - deterministic frame (fixed wire format)  
   - optional `seq`/`missed` Felder fuer Coalescing und Driftdiagnose

4. **Windowd VSync Controller**  
   - erstellt periodic timer aus Display-Mode  
   - blockierend im Eventloop  
   - tick nur auf Timer-Event

5. **Present Feedback Channel (neu, zwingend fuer Feinschliff)**  
   - `gpud` emittiert `PRESENT_DONE` Event mit `present_id/seq`  
   - `windowd` paced naechsten Tick anhand realer Present-Completion  
   - fallback auf Timer bei ausbleibendem Feedback

---

## 5) ABI- und Contract-Vorschlag (RFC-ready)

> Hinweis: Vor Implementierung als eigener RFC seed festziehen (neuer stabiler Contract).

### 5.1 Neue Syscalls (minimal, ausreichend)

```rust
// nexus-abi (proposal)
pub const SYSCALL_TIMER_CREATE: usize = 0x20;
pub const SYSCALL_TIMER_SET: usize = 0x21;
pub const SYSCALL_TIMER_CANCEL: usize = 0x22;
```

```rust
// timer_create: erstellt Timer-Capability, gebunden an notify endpoint
fn timer_create(notify_ep: Handle, flags: u32) -> Result<Handle, TimerError>;

// timer_set: absolute monotonic deadline, optional periodic interval
fn timer_set(timer: Handle, first_deadline_ns: u64, interval_ns: u64) -> Result<(), TimerError>;

// timer_cancel: disarm ohne Cap zu zerstoeren
fn timer_cancel(timer: Handle) -> Result<(), TimerError>;
```

### 5.2 Timer Event Wire Format (proposal)

```text
byte 0   : OP_TIMER_FIRED (fixed opcode)
byte 1-4 : timer_id (u32)
byte 5-8 : seq (u32)
byte 9-12: missed (u32)      // coalesced expirations
byte 13-20: deadline_ns (u64)
byte 21-28: fired_ns (u64)
```

### 5.3 Semantik

- `first_deadline_ns` ist **absolut** (monotonic nsec).  
- `interval_ns == 0` => OneShot, sonst Periodic.  
- Periodic-Rearm ist drift-frei: `next_deadline += interval_ns` (nicht `now + interval`).  
- Bei Queue-Backpressure: coalescing statt unbounded Event-Wachstum.

### 5.4 Present-Feedback Contract (zusaetzlicher Delta-Block)

```text
byte 0   : OP_PRESENT_DONE
byte 1-4 : present_id (u32)
byte 5-8 : seq (u32)
byte 9-16: submit_ns (u64)
byte 17-24: done_ns (u64)
byte 25  : status (0=ok, !=0 degraded/drop)
```

Regeln:

- Jeder von `windowd` eingereichte Present bekommt eine monotone `present_id`.
- `gpud` ackt completion asynchron ueber Event-Endpoint (nicht inline im request/reply Hotpath).
- `windowd` nutzt `done_ns - submit_ns` fuer Pacing/Backpressure-Entscheidungen.
- Bei fehlendem `PRESENT_DONE` innerhalb Budget: Timer-basierter fallback + counter/marker.

---

## 6) Kernel-Aenderungen (konkret)

### 6.1 Capability Layer

- Neuer `CapabilityKind::Timer { timer_id }`  
- Rechte mindestens: `SET`, `CANCEL`, `TRANSFER`  
- `cap_close` auf letzter Referenz disarmt und entfernt Timer-Eintrag

### 6.2 Timer Core

- Timer-Tabelle (`timer_id -> TimerState`)  
- Pro-Hart Deadline-Queue (oder global + lock, wenn simpler Start priorisiert wird)  
- O(log n) insert/update/remove

### 6.3 IRQ-Handling

1. now lesen  
2. alle faelligen Timer poppen  
3. Events enqueue (coalesced)  
4. periodic timer rearm  
5. naechstes globales/fruehestes Deadline wieder auf Hardware programmieren

### 6.4 Race/Determinism Regeln

- Queue-Operationen und IRQ-Rearm atomar gegenueber Interleave (kurzer kritischer Abschnitt)  
- Keine unbounded loops im IRQ-Pfad  
- Keine nondeterministischen Marker/Strings

---

## 7) `windowd` VSync-Pfad (sauber)

### 7.1 Ziel-Loop

```rust
let timer = timer_create(windowd_notify_ep, PERIODIC)?;
timer_set(timer, first_deadline_ns, refresh_interval_ns)?;

loop {
    match server.recv(Wait::Blocking) {
        Ok(msg) if is_timer_event(&msg) => {
            runtime.tick(nsec()?);
            runtime.flush_pending_damage()?;
        }
        Ok(msg) => {
            handle_input_or_rpc(msg)?;
            runtime.flush_pending_damage()?;
        }
        Err(err) => handle_ipc_error(err),
    }
}
```

### 7.2 Fallback-Strategie (wichtig fuer Rollout)

1. **Primary:** echte Timer-Capability  
2. **Fallback A:** `Wait::Timeout(frame_interval)` auf bestehendem Kernel-Deadline-Mechanismus  
3. **Fallback B (nur Bring-up):** alter `NonBlocking + yield_` Pfad

Damit ist bereits vor vollem Kernel-Timer-Objekt ein deutlich saubererer VSync-Pfad moeglich.

### 7.3 Warum Timer allein nicht reicht (entscheidendes Delta)

Ein Timer verbessert:

- Wakeup-Disziplin,
- Idle-CPU-Verbrauch,
- Grund-Jitter durch Polling.

Ein Timer loest **nicht allein**:

- wann der letzte Frame wirklich sichtbar wurde,
- wie viele Frames in flight sind,
- ob `gpud`/scanout bereits hinterherhaengt.

Fuer platform-class Smoothness-Level braucht es daher:

1. Timer fuer deterministic tick source, **plus**
2. Present-Completion Feedback fuer echte pacing closure.

---

## 8) Migrationsplan (phasenweise, risikoarm)

### Phase 0 - Contract Prep (0.5-1d)

- RFC seed fuer Timer-Capability + Wire-Contract  
- Fehlersemantik und Rechte finalisieren

### Phase 1 - Quick Win VSync Cleanup ohne neue Syscalls (1-2d)

- `windowd` von `NonBlocking + yield_` auf `Wait::Timeout(refresh_interval)` umstellen  
- Messbarer Gewinn bei Loop-Sauberkeit ohne Kernel-ABI-Break

### Phase 2 - Kernel Timer Capability (6-8d)

- Syscalls + CapKind + Timer queues + IRQ delivery  
- coalescing/backpressure handling

### Phase 3 - `windowd` auf echten Timer-Eventpfad (2-3d)

- `timer_create/timer_set` Nutzung  
- Blocking-loop finalisieren  
- Fallback A als degrade-path behalten

### Phase 4 - Validation/Hardening (3-4d)

- host + kernel + qemu proofs  
- jitter/latency baseline vs target

### Phase 5 - Present Feedback + Frame Pacing Closure (3-5d)

- `gpud` async `PRESENT_DONE` event path  
- `windowd` present_id tracking + in-flight budget (z. B. max 2)  
- adaptive pacing policy fuer heavy blur/glass Szenen

**Gesamt realistisch:** 15-20 Ingenieurtage (inkl. Tests und Risiko-Puffer).

---

## 9) Proof-Plan (kleinstes ehrliches Set)

### 9.1 Primaerer Proof

`windowd` bekommt periodische Timer-Events und praesentiert Frames ohne polling loop.

### 9.2 Test-Matrix

1. **Kernel Unit Tests**  
   - queue insert/remove/rearm  
   - drift-freie periodic schedule  
   - cancel/close semantics

2. **Kernel Integration Tests (QEMU)**  
   - create/set/cancel timer end-to-end  
   - timer event arrives at endpoint  
   - reject paths (`invalid handle`, `interval=0` edge, rights missing)

3. **Windowd Integration**  
   - blocking loop wakes on input + timer  
   - no `yield_` dependency fuer animation cadence  
   - frame pacing under input burst

4. **Present Feedback Integration (neu)**  
   - `present_id` monotonic und lueckenfrei  
   - `PRESENT_DONE` korreliert korrekt mit eingereichtem frame  
   - fallback ohne deadlock bei fehlender completion

5. **Determinism/Perf Check**  
   - stable markers/log labels  
   - bounded jitter target (e.g. p95 frame interval)

---

## 10) Risiken und Mitigation

| Risiko | Schwere | Mitigation |
|---|---|---|
| Queue/IRQ Race | Hoch | kurze kritische Abschnitte, klare lock order |
| Periodic Drift | Mittel | absolute deadlines + additive rearm |
| Event-Backpressure | Mittel | coalescing + `missed` counter |
| Kein Present-Feedback | Hoch | async PRESENT_DONE Kanal + in-flight cap |
| Timer Storm bei Fehlern | Mittel | bounded retries, kein unbounded requeue |
| Hardware detail (SBI latency) | Gering | fuer 60/120Hz ausreichend, SSTC optional spaeter |

---

## 11) Konkrete Aenderungspfade

### 11.1 Kernel

- `source/kernel/neuron/src/cap/*` (neuer Timer CapKind)  
- `source/kernel/neuron/src/syscall/api.rs` (timer syscalls)  
- `source/kernel/neuron/src/core/trap.rs` (timer expiry processing hook)  
- `source/kernel/neuron/src/hal/*` (bestehendes wakeup nutzen, ggf. minor glue)

### 11.2 ABI / Runtime

- `source/libs/nexus-abi/src/lib.rs` (syscall wrappers + error mapping)  
- `userspace/nexus-ipc/*` (optional helper fuer timer event frame decode)

### 11.3 Windowd

- `source/services/windowd/src/compositor/mod.rs` (event loop migration)  
- `source/services/windowd/src/compositor/runtime.rs` (timer event handling)

### 11.4 Gpud (neu fuer closure)

- `source/drivers/gpud/src/service.rs` (async present-done event emit)  
- `source/drivers/gpud/src/backend.rs` (present_id/seq completion plumbing)  
- optional `source/drivers/gpud/src/protocol.rs` (event opcode/wire contract)

---

## 12) Akzeptanzkriterien (Definition of Done)

- [ ] `windowd` laeuft ohne `NonBlocking + yield_` als Haupttaktquelle  
- [ ] Echte Timer-Capability ist per ABI nutzbar (`create/set/cancel`)  
- [ ] Periodic timer driftet nicht ueber laengere Laufzeit  
- [ ] Reject-path tests vorhanden und gruen  
- [ ] QEMU visible-bootstrap bleibt stabil und deterministic  
- [ ] Present-Feedback ist integriert (kein rein timerblindes pacing mehr)  
- [ ] Unter Glass/Blur Last ist Jitter sichtbar reduziert (definierte Messziele erreicht)  
- [ ] Dokumentation/RFC/Testing-Docs sind synchron zum implementierten Verhalten

---

## 13) Kurzfazit

Die Plattform ist **nahe dran**, aber noch nicht bei "echter Timer-Capability + sauberem VSync-Eventpfad + Present-closure".

Mit der hier definierten Architektur entsteht ein klarer, capability-nativer und testbarer Weg:
- kurzfristig sofort sauberer durch Timeout-basierten Loop,
- mittelfristig korrekt und voll reaktiv durch Timer-Capability,
- final sichtbar smooth durch Present-Feedback und echtes frame pacing.

---

## 14) Performance-Ziele fuer "spuerbar besser"

### 14.1 Mindestziele (muss)

- 60Hz Szenen: p95 frame interval <= 20ms, keine langen Burst-Stalls  
- 120Hz Szenen: p95 frame interval <= 10ms in leichten/mittleren Szenen  
- Unter Blur/Glass Last: kein dauerhafter "sawtooth" (stetiges hinterherlaufen)

### 14.2 Zielbild (soll)

- Frame pacing an Present-Done gekoppelt, max 1-2 frames in flight  
- Timer ist Referenz-Takt, Present ist Abschluss-Signal  
- adaptive degrade policy bei Lastspitzen (z. B. temporaer Blur-Qualitaet runter statt stottern)

### 14.3 Failure-Kriterien (nicht akzeptieren)

- "Kernel-Timer ist drin" aber visuell kein spuerbarer Gewinn  
- Nur synthetische Marker gruen, aber p95/p99 unter Last schlecht  
- Input-Latenz steigt deutlich wegen ueberstriktem blocking-ack pro frame
