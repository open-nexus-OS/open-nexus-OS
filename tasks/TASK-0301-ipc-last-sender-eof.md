# TASK-0301: IPC last-sender EOF + app self-exit on window close

- Status: Done
- Owners: @kernel-ipc-team / @runtime
- RFC: `docs/rfcs/RFC-0079-ipc-last-sender-eof.md`
- Related: RFC-0075 8f (process-image arena reclaim — this task makes it fire
  for parked GUI apps)

## Goal / stop condition

A closed app WINDOW terminates its app-host process (self-exit on
last-sender EOF), so the process image returns to the VMO arena. Proof: a
closed window emits the app's exit + `IMAGE-RECLAIM`; an open/close storm keeps
the arena bounded; every existing (non-opted) `recv` is unaffected.

## Phase 0 — kernel EOF contract (host reject-matrix)

- [x] `ipc::IpcError::PeerClosed` + recv errno 32 (EPIPE); `Endpoint.had_sender`
      latch (set on successful send + on recv-block scan finding a sender).
- [x] `CapTable::endpoint_send_cap_count(id)` (mirror `vmo_overlap_count`).
- [x] recv `IPC_SYS_EOF` opt-in: would-block + `had_sender` + zero live SEND
      caps → `PeerClosed`; else block.
- [x] `cap_close` of an `Endpoint(id)` SEND cap dropping the count to 0 wakes
      `id`'s recv-waiters.
- [x] `nexus-abi`: `IPC_SYS_EOF` flag + `PeerClosed` decode; `nexus-ipc` →
      `IpcError::Disconnected`.
- [x] Host reject-matrix (RFC-0079): no-flag blocks; never-had-sender blocks;
      had-sender + all-closed → EOF; pending message wins; non-last close no EOF.

## Phase 1 — app-host self-exit (QEMU)

- [x] app-host event loop opts into EOF; on `Disconnected`, self-exit.
- [x] Proof: closed window → app exit + `IMAGE-RECLAIM`; open/close storm keeps
      the arena bounded (no `VMO-POOL exhausted`).

## Non-goals

- Cross-task kill / reaper #29 (policyd-gated) — separate.
- Sender-task-exit prompt wake (lazy scan covers it; windowd uses cap_close).
