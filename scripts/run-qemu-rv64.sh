#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# Environment knobs:
#   RUN_TIMEOUT      – timeout(1) duration before QEMU is terminated (default: 30s)
#   RUN_UNTIL_MARKER – when "1", stop QEMU once a success UART marker is printed (default: 0)
#   QEMU_LOG_MAX     – maximum size of qemu.log after trimming (default: 52428800 bytes)
#   UART_LOG_MAX     – maximum size of uart.log after trimming (default: 10485760 bytes)
#   QEMU_LOG / UART_LOG – override log file paths.
#   INIT_LITE_LOG_TOPICS – comma separated init-lite log topic list (e.g. "svc-meta") propagated to the build script.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${TARGET:-riscv64imac-unknown-none-elf}
KERNEL_ELF=$ROOT/target/$TARGET/release/neuron-boot
KERNEL_BIN=$ROOT/target/$TARGET/release/neuron-boot.bin
INIT_ELF=$ROOT/target/$TARGET/release/init-lite
RUSTFLAGS_OS=${RUSTFLAGS_OS:---check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"}
export RUSTFLAGS="$RUSTFLAGS_OS"
RUN_TIMEOUT=${RUN_TIMEOUT:-90s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-0}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
QEMU_LOG=${QEMU_LOG:-qemu.log}
UART_LOG=${UART_LOG:-uart.log}
NEURON_BOOT_FEATURES=${NEURON_BOOT_FEATURES:-}
# Allow overriding the QEMU net backend (default: usernet) for opt-in harnesses.
QEMU_NETDEV=${QEMU_NETDEV:--netdev user,id=n0}
QEMU_NETDEV_DEVICE=${QEMU_NETDEV_DEVICE:--device virtio-net-device,netdev=n0}
QEMU_RNG_OBJECT=${QEMU_RNG_OBJECT:--object rng-random,id=rng0,filename=/dev/urandom}
QEMU_RNG_DEVICE=${QEMU_RNG_DEVICE:--device virtio-rng-device,rng=rng0}
QEMU_BLK_IMG=${QEMU_BLK_IMG:-$ROOT/build/blk.img}
QEMU_BLK_DRIVE=${QEMU_BLK_DRIVE:--drive if=none,file=$QEMU_BLK_IMG,format=raw,id=drvblk}
QEMU_BLK_DEVICE=${QEMU_BLK_DEVICE:--device virtio-blk-device,drive=drvblk}

join_by() {
  local IFS="$1"
  shift
  echo "$*"
}

set_env_var() {
  local name=$1
  local value=$2
  printf -v "$name" '%s' "$value"
  export "$name"
}

declare -a SERVICES=()

DEFAULT_SERVICE_LIST="keystored,rngd,policyd,logd,samgrd,bundlemgrd,updated,packagefsd,vfsd,execd,netstackd,virtioblkd,dsoftbusd,selftest-client"

prepare_service_payloads() {
  if [[ -z "${INIT_LITE_SERVICE_LIST:-}" ]]; then
    INIT_LITE_SERVICE_LIST=$DEFAULT_SERVICE_LIST
    export INIT_LITE_SERVICE_LIST
  fi

  if [[ -z "${INIT_LITE_SERVICE_LIST:-}" ]]; then
    SERVICES=()
  else
    IFS=',' read -r -a SERVICES <<<"$INIT_LITE_SERVICE_LIST"
  fi

  if [[ "${#SERVICES[@]}" -eq 0 ]]; then
    return
  fi

  for raw in "${SERVICES[@]}"; do
    local svc=${raw//[[:space:]]/}
    [[ -z "$svc" ]] && continue
    local svc_upper
    svc_upper=$(echo "$svc" | tr '[:lower:]' '[:upper:]' | tr '-' '_')

    local cargo_flags_var="INIT_LITE_SERVICE_${svc_upper}_CARGO_FLAGS"
    local -a cargo_args=(build -p "$svc" --target "$TARGET" --release)
    if [[ -n "${!cargo_flags_var:-}" ]]; then
      # shellcheck disable=SC2206 # intentionally split user-provided flags
      local extra_flags=(${!cargo_flags_var})
      cargo_args+=("${extra_flags[@]}")
    else
      cargo_args+=(--no-default-features --features os-lite)
    fi
    (cd "$ROOT" && RUSTFLAGS="$RUSTFLAGS_OS" cargo "${cargo_args[@]}")

    local elf_path="$ROOT/target/$TARGET/release/$svc"
    set_env_var "INIT_LITE_SERVICE_${svc_upper}_ELF" "$elf_path"
    local stack_var="INIT_LITE_SERVICE_${svc_upper}_STACK_PAGES"
    if [[ -z "${!stack_var:-}" ]]; then
      set_env_var "$stack_var" "8"
    fi
  done
}

# Continuous QEMU tracing can easily balloon into tens of gigabytes; trim the
# tail post-run to keep CI artifacts and local logs manageable.
trim_log() {
  local file=$1 max=$2
  if [[ -f "$file" ]]; then
    local sz
    sz=$(wc -c <"$file" || echo 0)
    if [[ "$sz" -gt "$max" ]]; then
      echo "[info] Trimming $file from ${sz} bytes to last $max bytes" >&2
      tail -c "$max" "$file" >"${file}.tmp" && mv "${file}.tmp" "$file"
    fi
  fi
}

# UART stream monitor that enforces the os-lite init readiness sequence before
# allowing RUN_UNTIL_MARKER=1 to stop QEMU. Expected order:
#   init: start
#   init: start <service>
#   init: up <service>
#   init: ready
#   packagefsd: ready
#   vfsd: ready
#   execd/selftest markers: elf load -> hello-elf -> e2e exec-elf -> exit0 start ->
#     child exited -> child exit ok
#   policy allow/deny probes
#   VFS stat/read/ebadf checks
monitor_uart() {
  local line
  local saw_init_start=0
  local saw_init_start_keystored=0
  local saw_init_start_policyd=0
  local saw_init_start_samgrd=0
  local saw_init_start_bundlemgrd=0
  local saw_init_start_packagefsd=0
  local saw_init_start_vfsd=0
  local saw_init_start_execd=0
  local saw_ready=0
  local saw_elf_ok=0
  local saw_exec_selftest=0
  local saw_child=0
  local saw_child_exit_start=0
  local saw_exit_log=0
  local saw_child_exit_ok=0
  local saw_execd_malformed=0
  local saw_exec_denied=0
  local saw_ipc_payload_roundtrip=0
  local saw_ipc_deadline_timeout=0
  local saw_nexus_ipc_kernel_loopback=0
  local saw_ipc_cap_move_reply=0
  local saw_ipc_sender_pid=0
  local saw_ipc_sender_service_id=0
  local saw_keystored_capmove=0
  local saw_kselftest_ipc_queue_full=0
  local saw_kselftest_ipc_bytes_full=0
  local saw_kselftest_ipc_global_bytes_budget=0
  local saw_kselftest_ipc_owner_bytes_budget=0
  local saw_kselftest_ipc_recv_waiter_fifo=0
  local saw_kselftest_ipc_send_waiter_fifo=0
  local saw_kselftest_ipc_send_unblock=0
  local saw_kselftest_ipc_endpoint_quota=0
  local saw_kselftest_ipc_close_wakes=0
  local saw_kselftest_ipc_owner_exit_wakes=0
  local saw_ipc_routing=0
  local saw_ipc_routing_pkg=0
  local saw_ipc_routing_policyd=0
  local saw_ipc_routing_bundlemgrd=0
  local saw_ipc_routing_samgrd=0
  local saw_samgrd_lookup=0
  local saw_samgrd_unknown=0
  local saw_samgrd_malformed=0
  local saw_samgrd_register=0
  local saw_ipc_routing_execd=0
  local saw_ipc_routing_keystored=0
  local saw_keystored_v1=0
  local saw_bundlemgrd_list=0
  local saw_bundlemgrd_malformed=0
  local saw_bundlemgrd_route_execd_denied=0
  local saw_pkgfs_ready=0
  local saw_vfsd_ready=0
  local saw_init_up_keystored=0
  local saw_init_up_policyd=0
  local saw_init_up_samgrd=0
  local saw_init_up_bundlemgrd=0
  local saw_init_up_packagefsd=0
  local saw_init_up_vfsd=0
  local saw_init_up_execd=0
  local saw_policy_allow=0
  local saw_policy_deny=0
  local saw_policy_spoof_denied=0
  local saw_policy_malformed=0
  local saw_vfs_stat=0
  local saw_vfs_read=0
  local saw_vfs_ebadf=0
  local saw_selftest_end=0
    while IFS= read -r line; do
    # Generic single-marker short-circuit: if RUN_UNTIL_MARKER is a non-zero,
    # non-"1" string, stop as soon as the line contains it.
    if [[ "$RUN_UNTIL_MARKER" != "0" && "$RUN_UNTIL_MARKER" != "1" ]]; then
      if [[ "$line" == *"$RUN_UNTIL_MARKER"* ]]; then
        echo "[info] Marker \"$RUN_UNTIL_MARKER\" seen – stopping QEMU" >&2
        pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
        break
      fi
    fi
    case "$line" in
      *"init: start keystored"*)
        saw_init_start_keystored=1
        ;;
      *"init: start policyd"*)
        saw_init_start_policyd=1
        ;;
      *"init: start samgrd"*)
        saw_init_start_samgrd=1
        ;;
      *"init: start bundlemgrd"*)
        saw_init_start_bundlemgrd=1
        ;;
      *"init: start packagefsd"*)
        saw_init_start_packagefsd=1
        ;;
      *"init: start vfsd"*)
        saw_init_start_vfsd=1
        ;;
      *"init: start execd"*)
        saw_init_start_execd=1
        ;;
      *"init: start"*)
        saw_init_start=1
        ;;
      *"init: ready"*)
        saw_ready=1
        ;;
      *"init: up keystored"*)
        saw_init_up_keystored=1
        ;;
      *"init: up policyd"*)
        saw_init_up_policyd=1
        ;;
      *"init: up samgrd"*)
        saw_init_up_samgrd=1
        ;;
      *"init: up bundlemgrd"*)
        saw_init_up_bundlemgrd=1
        ;;
      *"init: up packagefsd"*)
        saw_init_up_packagefsd=1
        ;;
      *"init: up vfsd"*)
        saw_init_up_vfsd=1
        ;;
      *"init: up execd"*)
        saw_init_up_execd=1
        ;;
      *"packagefsd: ready"*)
        saw_pkgfs_ready=1
        ;;
      *"vfsd: ready"*)
        saw_vfsd_ready=1
        ;;
      *"execd: elf load ok"*)
        saw_elf_ok=1
        ;;
      *"SELFTEST: e2e exec-elf ok"*)
        saw_exec_selftest=1
        ;;
      *"child: hello-elf"*)
        saw_child=1
        ;;
      *"child: exit0 start"*)
        saw_child_exit_start=1
        ;;
      *"execd: child exited pid="*)
        saw_exit_log=1
        ;;
      *"SELFTEST: child exit ok"*)
        saw_child_exit_ok=1
        ;;
      *"SELFTEST: execd malformed ok"*)
        saw_execd_malformed=1
        ;;
      *"SELFTEST: exec denied ok"*)
        saw_exec_denied=1
        ;;
      *"SELFTEST: ipc payload roundtrip ok"*)
        saw_ipc_payload_roundtrip=1
        ;;
      *"SELFTEST: ipc deadline timeout ok"*)
        saw_ipc_deadline_timeout=1
        ;;
      *"SELFTEST: nexus-ipc kernel loopback ok"*)
        saw_nexus_ipc_kernel_loopback=1
        ;;
      *"SELFTEST: ipc cap move reply ok"*)
        saw_ipc_cap_move_reply=1
        ;;
      *"SELFTEST: ipc sender pid ok"*)
        saw_ipc_sender_pid=1
        ;;
      *"SELFTEST: ipc sender service_id ok"*)
        saw_ipc_sender_service_id=1
        ;;
      *"SELFTEST: ipc routing policyd ok"*)
        saw_ipc_routing_policyd=1
        ;;
      *"SELFTEST: ipc routing bundlemgrd ok"*)
        saw_ipc_routing_bundlemgrd=1
        ;;
      *"SELFTEST: ipc routing samgrd ok"*)
        saw_ipc_routing_samgrd=1
        ;;
      *"SELFTEST: samgrd v1 lookup ok"*)
        saw_samgrd_lookup=1
        ;;
      *"SELFTEST: samgrd v1 register ok"*)
        saw_samgrd_register=1
        ;;
      *"SELFTEST: samgrd v1 unknown ok"*)
        saw_samgrd_unknown=1
        ;;
      *"SELFTEST: samgrd v1 malformed ok"*)
        saw_samgrd_malformed=1
        ;;
      *"SELFTEST: bundlemgrd v1 list ok"*)
        saw_bundlemgrd_list=1
        ;;
      *"SELFTEST: bundlemgrd v1 malformed ok"*)
        saw_bundlemgrd_malformed=1
        ;;
      *"SELFTEST: bundlemgrd route execd denied ok"*)
        saw_bundlemgrd_route_execd_denied=1
        ;;
      *"SELFTEST: ipc routing execd ok"*)
        saw_ipc_routing_execd=1
        ;;
      *"SELFTEST: ipc routing keystored ok"*)
        saw_ipc_routing_keystored=1
        ;;
      *"SELFTEST: keystored v1 ok"*)
        saw_keystored_v1=1
        ;;
      *"SELFTEST: keystored capmove ok"*)
        saw_keystored_capmove=1
        ;;
      *"KSELFTEST: ipc queue full ok"*)
        saw_kselftest_ipc_queue_full=1
        ;;
      *"KSELFTEST: ipc bytes full ok"*)
        saw_kselftest_ipc_bytes_full=1
        ;;
      *"KSELFTEST: ipc global bytes budget ok"*)
        saw_kselftest_ipc_global_bytes_budget=1
        ;;
      *"KSELFTEST: ipc owner bytes budget ok"*)
        saw_kselftest_ipc_owner_bytes_budget=1
        ;;
      *"KSELFTEST: ipc recv waiter fifo ok"*)
        saw_kselftest_ipc_recv_waiter_fifo=1
        ;;
      *"KSELFTEST: ipc send waiter fifo ok"*)
        saw_kselftest_ipc_send_waiter_fifo=1
        ;;
      *"KSELFTEST: ipc send unblock ok"*)
        saw_kselftest_ipc_send_unblock=1
        ;;
      *"KSELFTEST: ipc endpoint quota ok"*)
        saw_kselftest_ipc_endpoint_quota=1
        ;;
      *"KSELFTEST: ipc close wakes ok"*)
        saw_kselftest_ipc_close_wakes=1
        ;;
      *"KSELFTEST: ipc owner-exit wakes ok"*)
        saw_kselftest_ipc_owner_exit_wakes=1
        ;;
      *"SELFTEST: ipc routing ok"*)
        saw_ipc_routing=1
        ;;
      *"SELFTEST: ipc routing packagefsd ok"*)
        saw_ipc_routing_pkg=1
        ;;
      *"SELFTEST: policy allow ok"*)
        saw_policy_allow=1
        ;;
      *"SELFTEST: policy deny ok"*)
        saw_policy_deny=1
        ;;
      *"SELFTEST: policyd requester spoof denied ok"*)
        saw_policy_spoof_denied=1
        ;;
      *"SELFTEST: policy malformed ok"*)
        saw_policy_malformed=1
        ;;
      *"SELFTEST: vfs stat ok"*)
        saw_vfs_stat=1
        ;;
      *"SELFTEST: vfs read ok"*)
        saw_vfs_read=1
        ;;
      *"SELFTEST: vfs ebadf ok"*)
        saw_vfs_ebadf=1
        ;;
      *"SELFTEST: end"*)
        saw_selftest_end=1
        ;;
      *"I: after selftest"*|*"KSELFTEST: spawn ok"*|*"SELFTEST: ipc ok"*|*"SELFTEST: end"*)
        # When RUN_UNTIL_MARKER is a specific marker string, we only stop once that string is seen
        # (handled above). When RUN_UNTIL_MARKER=1, we stop once the full readiness/selftest set is
        # satisfied (handled below). Avoid stopping on these generic success markers, as it can
        # prevent userspace bring-up from running during phase-gated runs.
        ;;
      *"EXC: scause="*|*"PANIC "*|*"SELFTEST: fail"*|*"ILLEGAL"*|*"rx guard:"*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[warn] Exception/Panic marker detected – stopping QEMU early for triage" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi
        ;;
      *"LOCKDEP:"*|*"PT-VERIFY:"*|*"HEAP: "*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[warn] Kernel debug-hardening marker detected – stopping QEMU for triage" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi
        ;;
    esac

    if [[ "$RUN_UNTIL_MARKER" == "1" \
        && "$saw_init_start" -eq 1 && "$saw_init_start_keystored" -eq 1 && "$saw_init_start_policyd" -eq 1 && "$saw_init_start_samgrd" -eq 1 \
        && "$saw_init_start_bundlemgrd" -eq 1 && "$saw_init_start_packagefsd" -eq 1 && "$saw_init_start_vfsd" -eq 1 && "$saw_init_start_execd" -eq 1 \
        && "$saw_init_up_keystored" -eq 1 && "$saw_init_up_policyd" -eq 1 && "$saw_init_up_samgrd" -eq 1 && "$saw_init_up_bundlemgrd" -eq 1 \
        && "$saw_init_up_packagefsd" -eq 1 && "$saw_init_up_vfsd" -eq 1 && "$saw_init_up_execd" -eq 1 \
        && "$saw_ready" -eq 1 && "$saw_pkgfs_ready" -eq 1 && "$saw_vfsd_ready" -eq 1 \
        && "$saw_elf_ok" -eq 1 && "$saw_exec_selftest" -eq 1 && "$saw_child" -eq 1 && "$saw_child_exit_start" -eq 1 \
        && "$saw_exit_log" -eq 1 && "$saw_child_exit_ok" -eq 1 && "$saw_exec_denied" -eq 1 && "$saw_execd_malformed" -eq 1 && "$saw_policy_allow" -eq 1 && "$saw_policy_deny" -eq 1 && "$saw_policy_spoof_denied" -eq 1 && "$saw_policy_malformed" -eq 1 \
        && "$saw_ipc_payload_roundtrip" -eq 1 \
        && "$saw_ipc_deadline_timeout" -eq 1 \
        && "$saw_nexus_ipc_kernel_loopback" -eq 1 \
        && "$saw_ipc_cap_move_reply" -eq 1 \
        && "$saw_ipc_sender_pid" -eq 1 \
        && "$saw_ipc_sender_service_id" -eq 1 \
        && "$saw_keystored_capmove" -eq 1 \
        && "$saw_kselftest_ipc_queue_full" -eq 1 \
        && "$saw_kselftest_ipc_bytes_full" -eq 1 \
        && "$saw_kselftest_ipc_global_bytes_budget" -eq 1 \
        && "$saw_kselftest_ipc_owner_bytes_budget" -eq 1 \
        && "$saw_kselftest_ipc_recv_waiter_fifo" -eq 1 \
        && "$saw_kselftest_ipc_send_waiter_fifo" -eq 1 \
        && "$saw_kselftest_ipc_send_unblock" -eq 1 \
        && "$saw_kselftest_ipc_endpoint_quota" -eq 1 \
        && "$saw_kselftest_ipc_close_wakes" -eq 1 \
        && "$saw_kselftest_ipc_owner_exit_wakes" -eq 1 \
        && "$saw_ipc_routing_policyd" -eq 1 \
        && "$saw_ipc_routing_bundlemgrd" -eq 1 \
        && "$saw_bundlemgrd_list" -eq 1 \
        && "$saw_bundlemgrd_malformed" -eq 1 \
        && "$saw_bundlemgrd_route_execd_denied" -eq 1 \
        && "$saw_ipc_routing_keystored" -eq 1 \
        && "$saw_keystored_v1" -eq 1 \
        && "$saw_ipc_routing_samgrd" -eq 1 \
        && "$saw_samgrd_register" -eq 1 \
        && "$saw_samgrd_lookup" -eq 1 \
        && "$saw_samgrd_unknown" -eq 1 \
        && "$saw_samgrd_malformed" -eq 1 \
        && "$saw_ipc_routing_execd" -eq 1 \
        && "$saw_ipc_routing" -eq 1 \
        && "$saw_ipc_routing_pkg" -eq 1 \
        && "$saw_vfs_stat" -eq 1 && "$saw_vfs_read" -eq 1 && "$saw_vfs_ebadf" -eq 1 \
        && "$saw_selftest_end" -eq 1 ]]; then
      echo "[info] Success marker detected – stopping QEMU" >&2
      pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
      break
    fi
  done
}

finish() {
  local status=$1
  trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
  trim_log "$UART_LOG" "$UART_LOG_MAX"
  if [[ "$status" -eq 143 && "$RUN_UNTIL_MARKER" != "0" ]]; then
    echo "[info] QEMU stopped after success marker" >&2
    status=0
  fi
  if [[ "$status" -eq 124 ]]; then
    echo "[warn] QEMU terminated after exceeding timeout ($RUN_TIMEOUT)" >&2
  fi
  return "$status"
}

prepare_service_payloads

# Ensure a deterministic virtio-blk backing image exists for QEMU.
mkdir -p "$ROOT/build"
if [[ ! -f "$QEMU_BLK_IMG" ]]; then
  truncate -s 64M "$QEMU_BLK_IMG"
fi

# Always rebuild init-lite and kernel to pick up changes
(cd "$ROOT" && RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p init-lite --target "$TARGET" --release)
kernel_build() {
  local -a cargo_args=(build -p neuron-boot --target "$TARGET" --release)
  if [[ -n "$NEURON_BOOT_FEATURES" ]]; then
    cargo_args+=(--features "$NEURON_BOOT_FEATURES")
  fi
  (cd "$ROOT" && EMBED_INIT_ELF="$INIT_ELF" RUSTFLAGS="$RUSTFLAGS_OS" cargo "${cargo_args[@]}")
}

kernel_build

if [[ ! -f "$KERNEL_BIN" || "$KERNEL_BIN" -ot "$KERNEL_ELF" ]]; then
  # Use Rust's llvm-objcopy (works with all targets)
  OBJCOPY=$(find ~/.rustup/toolchains -name llvm-objcopy -type f 2>/dev/null | head -1)
  if [[ -z "$OBJCOPY" ]]; then
    echo "[error] llvm-objcopy not found. Install llvm-tools: rustup component add llvm-tools-preview" >&2
    exit 1
  fi
  "$OBJCOPY" -O binary "$KERNEL_ELF" "$KERNEL_BIN"
fi

rm -f "$QEMU_LOG" "$UART_LOG"

COMMON_ARGS=(
  -machine virt,aclint=on
  -cpu max
  -m 265M
  -smp "${SMP:-1}"
  -nographic
  -serial mon:stdio
  -icount 1,sleep=on
  -bios default
  -kernel "$KERNEL_BIN"
  # Networking: attach a virtio-net device on the virtio-mmio bus.
  # This is self-contained (user-mode net), requires no host TAP and remains deterministic enough
  # for marker-driven bring-up.
  ${QEMU_NETDEV}
  ${QEMU_NETDEV_DEVICE}
  ${QEMU_RNG_OBJECT}
  ${QEMU_RNG_DEVICE}
  ${QEMU_BLK_DRIVE}
  ${QEMU_BLK_DEVICE}
)

# Enable heavy QEMU tracing only when explicitly requested
if [[ "${QEMU_TRACE:-0}" == "1" ]]; then
  TRACE_FLAGS=${QEMU_TRACE_FLAGS:-int,mmu,unimp}
  COMMON_ARGS+=( -d "$TRACE_FLAGS" -D "$QEMU_LOG" )
fi

# Optional GDB stub for interactive debugging
if [[ "${QEMU_GDB:-0}" == "1" ]]; then
  COMMON_ARGS+=( -S -s -monitor none )
fi

status=0
if [[ "$RUN_UNTIL_MARKER" != "0" ]]; then
  set +e
  timeout --foreground "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@" \
    | tee >(monitor_uart) \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
else
  set +e
  timeout --foreground "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@" \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
fi

finish "$status"
