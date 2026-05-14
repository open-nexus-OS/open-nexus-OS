# Open Nexus OS

Rust microkernel OS. RISC-V. `no_std` userspace.

## Rules SSOT

`.cursor/` is single source of truth:
- `.cursor/rules/` — all rules (read them)
- `.cursor/current_state.md` — system state
- `.cursor/handoff/current.md` — live handoff

## DO

- Read `.cursor/current_state.md` + `.cursor/handoff/current.md` at session start.
- Read the task file. Read linked RFCs/ADRs.
- Plan before code. Contract before implementation.
- Host tests first. QEMU only when host can't cover the boundary.
- `just dep-gate` before claiming OS changes done.
- Markers: stable strings. No random IDs. No timestamps.
- Wrap-up: update RFC, CHANGELOG, `.cursor/` state, task boards.
- Debug: 3 tries max. Then stop. Update handoff. Fresh chat.

## DON'T

- Don't add prints/logs/markers in `source/kernel/**`.
- Don't emit `ready`/`ok` markers for stubs.
- Don't `unwrap`/`expect` on untrusted input in services.
- Don't log keys, secrets, credentials.
- Don't trust payload strings for identity (use kernel IPC sender id).
- Don't use `parking_lot`, `parking_lot_core`, `getrandom` in OS graph.
- Don't scan whole codebase. Read only task allowlist files.
- Don't guess past 3 failed debug attempts.

## Session start

``` text
@.cursor/current_state.md
@.cursor/handoff/current.md
@tasks/TASK-XXXX-*.md
```

## Session end

Update:
- `tasks/TASK-XXXX-*.md` (status)
- `.cursor/current_state.md` (overwrite, compressed)
- `.cursor/handoff/current.md` (proof + next step)
- `CHANGELOG.md` (Unreleased section)
- `tasks/IMPLEMENTATION-ORDER.md` + `tasks/STATUS-BOARD.md`

## Key commands

```bash
just test-all          # aggregate gate (fmt+lint+deny+host+e2e+miri+arch+kernel+smp)
just ci-network        # dhcp + quic-required + os2vm
just dep-gate          # forbidden crates check
just diag-host         # host build warnings
just diag-os           # OS build warnings
scripts/fmt-clippy-deny.sh  # fmt + clippy -D warnings + cargo-deny
make clean build test  # full from-scratch cycle
```
