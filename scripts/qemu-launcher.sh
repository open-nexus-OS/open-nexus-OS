#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# qemu-launcher.sh — QEMU launch only. No build, no marker verification.
#
# Called by: qemu-test.sh, just start, make run
#
# Environment (set by manifest list-env or caller):
#   NEXUS_SKIP_BUILD        – when "1", skip cargo build
#   NEXUS_DISPLAY_BOOTSTRAP – when "1", enable GPU + display
#   QEMU_DISPLAY_BACKEND    – gtk, none, headless
#   QEMU_SESSION_MODE       – proof | interactive
#   QEMU_MARKER_LEVEL       – proof | minimal | full
#   NEXUS_SELFTEST_MODE     – guest runtime mode via fw_cfg
#   NEXUS_SELFTEST_PROFILE  – guest runtime profile via fw_cfg
#   RUN_TIMEOUT             – timeout duration (0 = no timeout)
#   RUN_UNTIL_MARKER        – when "1", stop QEMU after success marker set
#   SMP                     – number of CPU cores
#   QEMU_NETDEV             – QEMU net backend
#   QEMU_NETDEV_DEVICE      – QEMU net device
#   QEMU_RNG_OBJECT         – QEMU RNG backend
#   QEMU_RNG_DEVICE         – QEMU RNG device
#   QEMU_GPU_DEVICE         – QEMU GPU device (optional, auto: virtio-gpu-device)
#   QEMU_BLK_DRIVE          – QEMU block drive
#   QEMU_BLK_DEVICE         – QEMU block device
#   QEMU_BLK_IMG            – QEMU block image path
#   QEMU_INPUT_AUTOINJECT   – when "1", enable QMP for visible input injection
#   QEMU_QMP_SOCKET         – QMP unix socket path
#   QEMU_PROOF_POINTER_SOURCE – mouse | tablet | keyboard | mixed
#   QEMU_ICOUNT_ARGS        – icount args (default: 1,sleep=on)
#   QEMU_NO_ICOUNT          – when "1", disable icount
#   QEMU_FORCE_LEGACY       – when "1", force legacy virtio-mmio
#   UART_LOG                – UART output log path
#   QEMU_LOG                – QEMU stderr log path
#   LOG_DIR                 – log directory
#   HYPOTHESIS_LOG          – hypothesis JSON log
#   RUN_ID                  – unique run identifier
#   BUILD_LOG               – build stderr log (for build.sh)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${TARGET:-riscv64imac-unknown-none-elf}

# --- Build (unless skipped) ---
NEXUS_SKIP_BUILD=${NEXUS_SKIP_BUILD:-0}
if [[ "$NEXUS_SKIP_BUILD" != "1" ]]; then
  # Initialise LOG_DIR and HYPOTHESIS_LOG BEFORE the build so that
  # build.sh can log H4 (build-error) entries.
  LOG_DIR=${LOG_DIR:-$ROOT/build/logs/manual--$(date +%Y-%m-%dT%H-%M-%S)}
  mkdir -p "$LOG_DIR"
  HYPOTHESIS_LOG=${HYPOTHESIS_LOG:-$LOG_DIR/hypothesis.json}
  RUN_ID=${RUN_ID:-"build-$(date +%s)-$$"}
  export LOG_DIR HYPOTHESIS_LOG RUN_ID
  source "$ROOT/scripts/build.sh"
  build_all
fi

# --- Paths ---
TARGET_ROOT=${CARGO_TARGET_DIR:-"$ROOT/target"}
KERNEL_ELF=$TARGET_ROOT/$TARGET/release/neuron-boot
KERNEL_BIN=$TARGET_ROOT/$TARGET/release/neuron-boot.bin
INIT_ELF=$TARGET_ROOT/$TARGET/release/init-lite

# --- Defaults ---
RUN_TIMEOUT=${RUN_TIMEOUT:-90s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-0}
QEMU_TIMEOUT_SIGNAL=${QEMU_TIMEOUT_SIGNAL:-TERM}
QEMU_SESSION_MODE=${QEMU_SESSION_MODE:-proof}
QEMU_MARKER_LEVEL=${QEMU_MARKER_LEVEL:-proof}
NEXUS_DISPLAY_BOOTSTRAP=${NEXUS_DISPLAY_BOOTSTRAP:-0}
QEMU_INPUT_AUTOINJECT=${QEMU_INPUT_AUTOINJECT:-0}
QEMU_QMP_SOCKET=${QEMU_QMP_SOCKET:-$ROOT/build/qemu.qmp}
QEMU_PROOF_POINTER_SOURCE=${QEMU_PROOF_POINTER_SOURCE:-mouse}
QEMU_GPU_XRES=${QEMU_GPU_XRES:-1280}
QEMU_GPU_YRES=${QEMU_GPU_YRES:-800}
QEMU_ICOUNT_ARGS=${QEMU_ICOUNT_ARGS:-"1,sleep=on"}
# SMP > 1 runs MTTCG (real parallel vCPUs): icount serializes all harts onto
# one round-robin thread. The interactive default is SMP=4 (see below), so
# MTTCG is the interactive default too; the deterministic icount profile
# applies when SMP=1 is chosen (qemu-test.sh's deterministic profiles) and
# can be forced with QEMU_NO_ICOUNT=0.
if [[ -z "${QEMU_NO_ICOUNT:-}" && "${SMP:-4}" != "1" ]]; then
  QEMU_NO_ICOUNT=1
fi
QEMU_NO_ICOUNT=${QEMU_NO_ICOUNT:-0}
QEMU_FORCE_LEGACY=${QEMU_FORCE_LEGACY:-0}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
LOG_DIR=${LOG_DIR:-$ROOT/build/logs/manual--$(date +%Y-%m-%dT%H-%M-%S)}
mkdir -p "$LOG_DIR"
# Keep build/logs/latest pointing at THIS run so `latest/uart.log` is never stale
# (the recurring "I see the old uart" trap). `-fn` replaces the existing symlink in place.
ln -sfn "$LOG_DIR" "$ROOT/build/logs/latest" 2>/dev/null || true
UART_LOG=${UART_LOG:-$LOG_DIR/uart.log}
QEMU_LOG=${QEMU_LOG:-$LOG_DIR/qemu.stderr}
HYPOTHESIS_LOG=${HYPOTHESIS_LOG:-$LOG_DIR/hypothesis.json}
RUN_ID=${RUN_ID:-"qemu-$(date +%s)-$$"}

QEMU_NETDEV=${QEMU_NETDEV:--netdev user,id=n0}
QEMU_NETDEV_DEVICE=${QEMU_NETDEV_DEVICE:--device virtio-net-device,netdev=n0}
QEMU_RNG_OBJECT=${QEMU_RNG_OBJECT:--object rng-random,id=rng0,filename=/dev/urandom}
QEMU_RNG_DEVICE=${QEMU_RNG_DEVICE:--device virtio-rng-device,rng=rng0}
GPU_MODE=${GPU_MODE:-mmio}
QEMU_DISPLAY_BACKEND=${QEMU_DISPLAY_BACKEND:-gtk}
QEMU_BLK_IMG=${QEMU_BLK_IMG:-$ROOT/build/blk.img}
QEMU_BLK_DRIVE=${QEMU_BLK_DRIVE:--drive if=none,file=$QEMU_BLK_IMG,format=raw,id=drvblk}
QEMU_BLK_DEVICE=${QEMU_BLK_DEVICE:--device virtio-blk-device,drive=drvblk}
QEMU_BLK_LOCK_FILE=${QEMU_BLK_LOCK_FILE:-"$ROOT/build/.qemu-blk.lock"}
QEMU_BLK_LOCK_WAIT=${QEMU_BLK_LOCK_WAIT:-180}
# Second virtio-blk device: the nxfs `/data` user-data volume (ADR-0044 /
# TASK-0293). statefs owns blk.img (device 1); nxfs owns data.img (device 2).
QEMU_DATA_IMG=${QEMU_DATA_IMG:-$ROOT/build/data.img}
QEMU_DATA_DRIVE=${QEMU_DATA_DRIVE:--drive if=none,file=$QEMU_DATA_IMG,format=raw,id=drvdata}
QEMU_DATA_DEVICE=${QEMU_DATA_DEVICE:--device virtio-blk-device,drive=drvdata}
# NEXUS_KEEP_BLK=1 preserves both images across launches so cold-boot
# persistence can be proven (default: wipe per boot for deterministic runs).
NEXUS_KEEP_BLK=${NEXUS_KEEP_BLK:-0}

INTERACTIVE_READY_SENTINEL=${INTERACTIVE_READY_SENTINEL:-$ROOT/build/.interactive-scene-ready}

# --- Hypothesis debug_log ---
debug_log() {
  if [[ ! -f "$HYPOTHESIS_LOG" ]]; then return 0; fi
  local hypothesis_id=$1 location=$2 message=$3 data=$4 ts
  ts=$(date +%s%3N 2>/dev/null || echo 0)
  printf '{"runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
    "$RUN_ID" "$hypothesis_id" "$location" "$message" "$data" "$ts" >>"$HYPOTHESIS_LOG" 2>/dev/null || true
}

# --- objcopy kernel ELF to binary ---
prepare_kernel_bin() {
  if [[ ! -f "$KERNEL_BIN" || "$KERNEL_BIN" -ot "$KERNEL_ELF" ]]; then
    local objcopy=""
    local candidate
    for candidate in \
      "$HOME"/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-objcopy \
      "$HOME"/.rustup/toolchains/*/bin/llvm-objcopy
    do
      if [[ -x "$candidate" ]]; then
        objcopy="$candidate"
        break
      fi
    done
    if [[ -z "$objcopy" ]]; then
      echo "[error] llvm-objcopy not found. Install: rustup component add llvm-tools-preview" >&2
      exit 1
    fi
    "$objcopy" -O binary "$KERNEL_ELF" "$KERNEL_BIN"
  fi
}

# --- blk image ---
prepare_blk_image() {
  mkdir -p "$ROOT/build"
  exec 9>"$QEMU_BLK_LOCK_FILE"
  if ! flock -w "$QEMU_BLK_LOCK_WAIT" 9; then
    echo "[error] Timed out waiting for blk image lock: $QEMU_BLK_LOCK_FILE" >&2
    exit 1
  fi
  if [[ "$NEXUS_KEEP_BLK" == "1" ]]; then
    # Cold-boot persistence: keep existing images; create only if missing.
    [[ -f "$QEMU_BLK_IMG" ]] || truncate -s 64M "$QEMU_BLK_IMG"
    [[ -f "$QEMU_DATA_IMG" ]] || truncate -s 64M "$QEMU_DATA_IMG"
  else
    rm -f "$QEMU_BLK_IMG" "$QEMU_DATA_IMG"
    truncate -s 64M "$QEMU_BLK_IMG"
    truncate -s 64M "$QEMU_DATA_IMG"
  fi
}

cleanup_blk_lock() {
  flock -u 9 2>/dev/null || true
  exec 9>&- 2>/dev/null || true
}

cleanup_qmp() {
  rm -f "$QEMU_QMP_SOCKET"
}

cleanup_input_injector() {
  if [[ -n "${INPUT_INJECT_PID:-}" ]]; then
    kill "$INPUT_INJECT_PID" >/dev/null 2>&1 || true
    wait "$INPUT_INJECT_PID" >/dev/null 2>&1 || true
    INPUT_INJECT_PID=""
  fi
}

trap 'cleanup_input_injector; cleanup_qmp; cleanup_blk_lock' EXIT

# --- trim log tail ---
trim_log() {
  local file=$1 max=$2 sz
  if [[ -f "$file" ]]; then
    sz=$(wc -c <"$file" || echo 0)
    if [[ "$sz" -gt "$max" ]]; then
      echo "[info] Trimming $file from ${sz} bytes to last $max bytes" >&2
      tail -c "$max" "$file" >"${file}.tmp" && mv "${file}.tmp" "$file"
    fi
  fi
}

# --- Build QEMU command ---
build_qemu_args() {
  local -a args=()
  local -a input_args=()
  # Interactive default is SMP=4 + MTTCG (the product configuration). The
  # soft-realtime plan made this viable and MEASURABLE: declarative cpu
  # placement (display/input chain on cpu0, background on cpu1-3), phased
  # vmo/exec syscalls + a lock-free syscall class + cpu0 right-of-way at the
  # BKL (boot max wait 90.8ms -> ~6ms, max hold 90ms -> ~4ms), gated by
  # `KSELFTEST: bkl budget ok` in the SMP profile.
  # QEMU_ACLINT=off: fall back to the classic SiFive CLINT MSWI IPI path.
  # OpenSBI 1.7's HSM hart_start via ACLINT-MSWI was observed to LOSE a hart
  # under MTTCG (hart marked STARTED, never reaches the kernel entry).
  args+=(-machine "virt,aclint=${QEMU_ACLINT:-on}" -cpu max -m 320M -smp "${SMP:-4}" -bios default)
  args+=(-kernel "$KERNEL_BIN")

  # Display mode
  if [[ "$NEXUS_DISPLAY_BOOTSTRAP" == "1" ]]; then
    local display_backend="$QEMU_DISPLAY_BACKEND"
    [[ "$display_backend" == "gtk" ]] && display_backend="gtk,show-menubar=off,zoom-to-fit=off"
    if [[ "$GPU_MODE" == "virgl" ]]; then
      # Windowed virgl: a GL-accelerated display backend so virglrenderer comes
      # up against the window's GL context, plus the GL-capable GPU device
      # (MMIO transport matches gpud). gpud must be built with the `virgl`
      # feature (scripts/build.sh wires that when GPU_MODE=virgl). This is the
      # visible counterpart to the headless egl-headless virgl proof below.
      # Any windowed backend (gtk/sdl) needs gl=on for virgl; honour an
      # explicit gl= option if the caller already set one.
      if [[ "$display_backend" != *,gl=* && "$display_backend" != egl-headless* ]]; then
        display_backend="${display_backend},gl=on"
      fi
      args+=(-display "$display_backend" -serial mon:stdio)
      args+=(-device "virtio-gpu-gl-device,max_outputs=1,xres=${QEMU_GPU_XRES},yres=${QEMU_GPU_YRES}")
    elif [[ "$GPU_MODE" == "pci" ]]; then
      # PCIe virtio-gpu connects correctly to the QEMU display backend.
      # Bus auto-assignment works on riscv64 virt machine's built-in PCIe.
      args+=(-display "$display_backend" -serial mon:stdio)
      args+=(-device "virtio-gpu-pci,max_outputs=1,xres=${QEMU_GPU_XRES},yres=${QEMU_GPU_YRES}")
    else
      args+=(-display "$display_backend" -serial mon:stdio)
      args+=(-device "virtio-gpu-device,max_outputs=1,xres=${QEMU_GPU_XRES},yres=${QEMU_GPU_YRES}")
    fi
    # Visible bootstrap needs deterministic virtio-input MMIO devices so
    # hidrawd can own the real input-driver hop under the selected proof mode.
    input_args+=(-device virtio-keyboard-device)
    case "$QEMU_PROOF_POINTER_SOURCE" in
      mouse|"")
        input_args+=(-device virtio-mouse-device)
        ;;
      tablet)
        input_args+=(-device virtio-tablet-device)
        ;;
      keyboard)
        ;;
      mixed)
        input_args+=(-device virtio-mouse-device -device virtio-tablet-device)
        ;;
      *)
        echo "[error] Unknown QEMU_PROOF_POINTER_SOURCE='$QEMU_PROOF_POINTER_SOURCE' (supported: mouse tablet keyboard mixed)" >&2
        exit 1
        ;;
    esac
  elif [[ "$GPU_MODE" == "virgl" ]]; then
    # virgl 3D proof: virtio-gpu-gl needs a GL-capable display backend to bring
    # up virglrenderer on the host, so we use egl-headless (no window, host EGL
    # context) instead of -nographic. The MMIO `-gl-device` matches gpud's
    # virtio-mmio transport; gpud must be built with the `virgl` feature.
    args+=(-display egl-headless -serial mon:stdio)
    args+=(-device "virtio-gpu-gl-device,max_outputs=1,xres=${QEMU_GPU_XRES},yres=${QEMU_GPU_YRES}")
  else
    args+=(-nographic -serial mon:stdio)
    if [[ "$GPU_MODE" == "pci" ]]; then
      args+=(-device "virtio-gpu-pci,max_outputs=1,xres=${QEMU_GPU_XRES},yres=${QEMU_GPU_YRES}")
    else
      args+=(-device "virtio-gpu-device,max_outputs=1,xres=${QEMU_GPU_XRES},yres=${QEMU_GPU_YRES}")
    fi
  fi

  # QMP for visible input injection
  if [[ "$QEMU_INPUT_AUTOINJECT" == "1" ]]; then
    rm -f "$QEMU_QMP_SOCKET"
    args+=(-qmp "unix:$QEMU_QMP_SOCKET,server=on,wait=off")
  fi

  # fw_cfg: selftest mode + profile
  if [[ -n "${NEXUS_SELFTEST_MODE:-}" ]]; then
    args+=(-fw_cfg "name=opt/org.open-nexus/selftest-mode,string=${NEXUS_SELFTEST_MODE}")
  fi
  if [[ -n "${NEXUS_SELFTEST_PROFILE:-}" ]]; then
    args+=(-fw_cfg "name=opt/org.open-nexus/selftest-profile,string=${NEXUS_SELFTEST_PROFILE}")
  fi
  # fw_cfg: authoritative display mode (RFC-0074 / ADR-0050). The compositor
  # OWNS the mode; this is the configured source of truth the guest commands
  # onto the scanout, so the GTK window-realize race can never latch a
  # transient wrong size. Same value as the virtio-gpu device's xres/yres.
  args+=(-fw_cfg "name=opt/org.open-nexus/display-mode,string=${QEMU_GPU_XRES}x${QEMU_GPU_YRES}")

  # virtio-mmio modern
  if [[ "${QEMU_FORCE_LEGACY:-0}" == "1" ]]; then
    args+=(-global virtio-mmio.force-legacy=on)
  else
    args+=(-global virtio-mmio.force-legacy=off)
  fi

  # icount
  if [[ "${QEMU_NO_ICOUNT:-0}" != "1" ]]; then
    args+=(-icount "$QEMU_ICOUNT_ARGS")
  fi

  # Peripherals
  args+=(${QEMU_NETDEV} ${QEMU_NETDEV_DEVICE})
  args+=(${QEMU_RNG_OBJECT} ${QEMU_RNG_DEVICE})
  args+=(${QEMU_BLK_DRIVE} ${QEMU_BLK_DEVICE})
  args+=("${input_args[@]}")
  # nxfs `/data` device LAST: keep it after the virtio-input devices so their
  # virtio-mmio transport slots stay identical to the single-blk layout (a
  # 2nd device inserted before them shifts input slots and breaks the pointer).
  args+=(${QEMU_DATA_DRIVE} ${QEMU_DATA_DEVICE})

  # Debug/proof hook: extra QEMU arguments (e.g. "-vnc :77" to read back GL
  # scanouts for screendump verification on headless hosts).
  if [[ -n "${QEMU_EXTRA_ARGS:-}" ]]; then
    # shellcheck disable=SC2206
    args+=(${QEMU_EXTRA_ARGS})
  fi

  printf '%s\n' "${args[@]}"
}

start_visible_input_injector() {
  if [[ "$QEMU_INPUT_AUTOINJECT" != "1" ]]; then
    return 0
  fi

  local profile_mouse=0
  local profile_touch=0
  local profile_kbd=1
  case "$QEMU_PROOF_POINTER_SOURCE" in
    mouse|"")
      profile_mouse=1
      profile_touch=0
      ;;
    tablet)
      profile_mouse=0
      profile_touch=1
      ;;
    keyboard)
      profile_mouse=0
      profile_touch=0
      ;;
    mixed)
      profile_mouse=1
      profile_touch=1
      ;;
    *)
      echo "[error] Unknown QEMU_PROOF_POINTER_SOURCE='$QEMU_PROOF_POINTER_SOURCE' for injector" >&2
      exit 1
      ;;
  esac

  QEMU_UART_LOG_PATH="$UART_LOG" \
  NEXUS_PROFILE_INPUT_MOUSE="$profile_mouse" \
  NEXUS_PROFILE_INPUT_TOUCH="$profile_touch" \
  NEXUS_PROFILE_INPUT_KBD="$profile_kbd" \
  QEMU_SESSION_MODE="$QEMU_SESSION_MODE" \
  LOG_DIR="$LOG_DIR" \
  HYPOTHESIS_LOG="$HYPOTHESIS_LOG" \
  RUN_ID="$RUN_ID" \
  python3 "$ROOT/tools/qmp_visible_input_inject.py" "$QEMU_QMP_SOCKET" >>"$QEMU_LOG" 2>&1 &
  INPUT_INJECT_PID=$!
}

# --- Monitor UART for early exit ---
monitor_uart_stream() {
  local line saw_init_start=0 saw_ready=0
  local saw_kself_ok=0
  while IFS= read -r line; do
    echo "$line"
    case "$line" in
      *"KSELFTEST: spawn reasons ok"*) saw_kself_ok=1 ;;
      *"init: start"*) saw_init_start=1 ;;
      *"init: ready"*) saw_ready=1 ;; 
      *"windowd: interactive scene ready"*)
        printf 'ready\n' >"$INTERACTIVE_READY_SENTINEL" ;;
      *"EXC: scause="*|*"PANIC "*|*"SELFTEST: fail"*|*"ILLEGAL"*|*"rx guard:"*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[warn] Exception/Panic marker detected – stopping QEMU early for triage" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi ;;
      *"LOCKDEP:"*|*"PT-VERIFY:"*|*"HEAP: "*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[warn] Kernel debug-hardening marker detected – stopping QEMU for triage" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi ;;
    esac
    # RUN_UNTIL_MARKER=1: stop when init: ready + grace period passes
    # so that all service readiness markers and routing probes flush to UART.
    if [[ "$RUN_UNTIL_MARKER" == "1" && "$saw_kself_ok" -eq 1 && "$saw_init_start" -eq 1 && "$saw_ready" -eq 1 ]]; then
      # Grace period: a FIXED window after `init: ready` for service-readiness +
      # routing + selftest markers to flush. The dominant cost is the SELFTEST
      # LADDER (bringup→routing→ota→…), which is IDENTICAL in every lane — only
      # the display backend differs. The old 30s default was sized for the fast
      # 2D DISPLAY bringup, not the ladder length, so the ladder was cut off
      # before its later phases: with selftest-client no longer Idle-starved the
      # ladder actually runs, and reaching the OTA phase (`bundlemgrd: slot a
      # active`, phase 3, gated behind bringup's slow statefs-persist disk I/O)
      # needs ~90s — measured, the full ladder completes well within it. So the
      # grace is 90s regardless of GPU_MODE (the ladder, not the display, is the
      # limiter). `QEMU_READY_GRACE_SECS` still overrides for ad-hoc runs.
      local grace_secs="${QEMU_READY_GRACE_SECS:-90}"
      local start_nsec
      start_nsec=$(date +%s 2>/dev/null || echo 0)
      # Early exit: the moment the ladder's FINAL marker flushes, stop after a
      # short tail-drain instead of sitting out the fixed window (proof lanes
      # used to waste up to ~85s per run; the fixed window stays as the
      # fallback when the ladder never finishes).
      local ladder_done_at=0
      while true; do
        local now
        now=$(date +%s 2>/dev/null || echo 0)
        [[ $(( now - start_nsec )) -ge "$grace_secs" ]] && break
        if [[ "$ladder_done_at" -ne 0 && $(( now - ladder_done_at )) -ge 2 ]]; then
          echo "[info] ladder end marker seen – early stop" >&2
          break
        fi
        # Read as fast as lines arrive (max 100ms silence = buffer empty)
        IFS= read -r -t 0.1 line 2>/dev/null || true
        if [[ -n "$line" ]]; then
          echo "$line"
          case "$line" in
            *"SELFTEST: ui resize ok"*|*"SELFTEST: Completed"*) ladder_done_at=$now ;;
          esac
        fi
      done
      echo "[info] init: ready seen, grace period done – stopping QEMU" >&2
      pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
      break
    fi
  done
}

# --- Main ---
prepare_blk_image
prepare_kernel_bin
rm -f "$QEMU_LOG" "$UART_LOG"

# Hypothesis: host resources pre-launch
debug_log "H14" "scripts/qemu-launcher.sh:host-resources-pre" "host resource snapshot before qemu launch" \
  "{\"target_root\":\"$TARGET_ROOT\",\"kernel_elf\":\"$KERNEL_ELF\",\"init_elf\":\"$INIT_ELF\"}"

# Hypothesis: resolved config
debug_log "H1" "scripts/qemu-launcher.sh:resolved-config" "resolved qemu config before launch" \
  "{\"display_bootstrap\":\"$NEXUS_DISPLAY_BOOTSTRAP\",\"display_backend\":\"$QEMU_DISPLAY_BACKEND\",\"gpu_xres\":\"$QEMU_GPU_XRES\",\"gpu_yres\":\"$QEMU_GPU_YRES\",\"pointer_source\":\"$QEMU_PROOF_POINTER_SOURCE\",\"smp\":\"${SMP:-1}\",\"timeout\":\"$RUN_TIMEOUT\"}"

# Build QEMU args
mapfile -t QEMU_ARGS < <(build_qemu_args)
echo "[info] QEMU args: ${QEMU_ARGS[*]}" >&2

# Launch QEMU
start_visible_input_injector
if [[ "$RUN_TIMEOUT" == "0" ]]; then
  stdbuf -oL -eL qemu-system-riscv64 "${QEMU_ARGS[@]}" > >(monitor_uart_stream | tee "$UART_LOG") 2>"$QEMU_LOG"
  qemu_status=$?
else
  timeout --signal="$QEMU_TIMEOUT_SIGNAL" --foreground "$RUN_TIMEOUT" \
    stdbuf -oL -eL qemu-system-riscv64 "${QEMU_ARGS[@]}" > >(monitor_uart_stream | tee "$UART_LOG") 2>"$QEMU_LOG"
  qemu_status=$?
fi

# Post-run cleanup
trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
trim_log "$UART_LOG" "$UART_LOG_MAX"
rm -f "$INTERACTIVE_READY_SENTINEL"

# Tolerate exit 124 (timeout from `timeout` command) and 143 (SIGTERM from early-exit).
# When we killed QEMU ourselves (RUN_UNTIL_MARKER=1), treat as success.
if [[ "$qemu_status" -eq 143 || "$qemu_status" -eq 124 ]]; then
  echo "[info] QEMU stopped (signal/timeout), exit=$qemu_status" >&2
  qemu_status=0
fi

# Hypothesis: QEMU result
debug_log "J" "scripts/qemu-launcher.sh:qemu-result" "qemu exit status" \
  "{\"exit_code\":$qemu_status,\"uart_exists\":$([[ -f "$UART_LOG" ]] && echo true || echo false)}"

exit "$qemu_status"
