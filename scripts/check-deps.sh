#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Post-condition check for `make initial-setup` — "is this host able to
#          run make build/test/run, just start and just test-all?". Deliberately
#          checks CAPABILITIES (binaries, toolchain components, QEMU display
#          backends, fetched inputs), never package names: names differ per
#          distro AND per release, capabilities do not. This is the only
#          statement about a host that means the same thing on Ubuntu, Fedora
#          and Arch. Needs no sudo and runs in seconds.
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable
# TEST_COVERAGE: Self-verifying (it IS the check); exercised by `make doctor`
# ADR: docs/architecture/02-selftest-and-ci.md
#
# Usage:
#   scripts/check-deps.sh                 # full report, exit 1 on any failure
#   scripts/check-deps.sh --workspace-only  # only the $HOME/ownership gate
#   scripts/check-deps.sh --quiet         # only failures
#
# Env:
#   GUI=0      skip the GTK/OpenGL display checks (headless host)
#   PODMAN=0   skip the rootless-podman checks (host-only workflow)
#   BOARD=0    skip the board flash/serial tool checks (no reference board here)
#
# Exit codes:
#   0  everything required is present
#   1  at least one required check failed

set -uo pipefail   # NOT -e: this script's whole job is to keep going and report

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

NIGHTLY="nightly-2025-01-15"     # keep in sync with rust-toolchain.toml
RV_TARGET="riscv64imac-unknown-none-elf"
WANT_GUI="${GUI:-1}"
WANT_PODMAN="${PODMAN:-1}"
WANT_BOARD="${BOARD:-1}"

WORKSPACE_ONLY=0
QUIET=0
for arg in "$@"; do
  case "$arg" in
    --workspace-only) WORKSPACE_ONLY=1 ;;
    --quiet|-q)       QUIET=1 ;;
    -h|--help)        sed -n '17,30p' "$0"; exit 0 ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

FAILURES=0
WARNINGS=0

# rustup puts its proxies in ~/.cargo/bin and appends that to the shell PROFILE,
# which an already-running shell has not read. Look there regardless, so a tool
# that IS installed is never reported as missing — that sends people
# re-installing what they already have. The PATH itself is checked once, below.
CARGO_BIN="$HOME/.cargo/bin"
PATH_HAS_CARGO_BIN=1
case ":$PATH:" in
  *":$CARGO_BIN:"*) ;;
  *) PATH_HAS_CARGO_BIN=0; export PATH="$CARGO_BIN:$PATH" ;;
esac

ok()   { [ "$QUIET" = 1 ] || printf '  \033[1;32m[ok]\033[0m   %s\n' "$*"; }
bad()  { printf '  \033[1;31m[FAIL]\033[0m %s\n' "$1"; printf '         fix: %s\n' "$2"; FAILURES=$((FAILURES + 1)); }
soft() { printf '  \033[1;33m[warn]\033[0m %s\n' "$1"; printf '         fix: %s\n' "$2"; WARNINGS=$((WARNINGS + 1)); }
head_() { [ "$QUIET" = 1 ] || printf '\n\033[1m%s\033[0m\n' "$*"; }

# --------- 1. workspace location -------------------------------------------
# Rootless podman maps the workspace into a user namespace and cargo writes
# target/ as the invoking user; both need a user-owned path. A checkout under
# /opt or /srv fails deep inside a podman run with an unhelpful error, so catch
# it here.
head_ "Workspace"
# The requirement is about the CHECKOUT (under the developer's home, owned by
# them), not about how the command was typed. Under `sudo make initial-setup`
# $HOME is /root and `id -un` is root, so a naive comparison reports a correct
# checkout as misplaced and proposes chowning it to root. Resolve the invoking
# user first (SUDO_USER → passwd) and judge the checkout against THEIR home.
me="${SUDO_USER:-$(id -un)}"
me_home="$(getent passwd "$me" 2>/dev/null | cut -d: -f6)"
[ -n "$me_home" ] || me_home="${HOME:-}"
if [ -z "$me_home" ]; then
  bad "cannot determine the home directory of $me" "run this as a normal login user"
else
  case "$REPO_ROOT/" in
    "$me_home"/*) ok "checkout is under $me's home ($REPO_ROOT)" ;;
    *) bad "checkout is at $REPO_ROOT, which is NOT under $me's home ($me_home)" \
           "move it (e.g. $me_home/open-nexus-OS) — rootless podman + cargo caches need user-owned paths" ;;
  esac
fi

owner="$(stat -c '%U' "$REPO_ROOT" 2>/dev/null || echo '?')"
if [ "$owner" = "$me" ]; then
  ok "checkout is owned by $me"
else
  bad "checkout is owned by '$owner', not $me" \
      "sudo chown -R $me: $REPO_ROOT"
fi

# The setup itself must run AS that user, not as root: rustup installs into
# ~/.cargo of whoever runs it, cargo writes target/ and its caches as that
# user, and podman's rootless mode is a per-user property. The scripts call
# sudo themselves for the few steps that need it (packages, udev rule, group).
if [ "$(id -u)" -eq 0 ]; then
  bad "running as root${SUDO_USER:+ (sudo from $SUDO_USER)} — rustup/cargo/podman state would be created for root, not for $me" \
      "run it without sudo:  cd $REPO_ROOT && make initial-setup   (the scripts ask for sudo where they need it)"
  exit 1
fi

if [ "$WORKSPACE_ONLY" = 1 ]; then
  [ "$FAILURES" -eq 0 ] && exit 0
  exit 1
fi

# --------- 2. binaries on PATH ----------------------------------------------
# Each entry: binary | what needs it | how to get it
need_bin() {
  local bin=$1 why=$2 fix=$3
  if command -v "$bin" >/dev/null 2>&1; then
    ok "$bin ($why)"
  else
    bad "$bin missing — needed by $why" "$fix"
  fi
}

head_ "Host tools"
need_bin git    "everything"                    "scripts/install-deps.sh"
need_bin make   "the make spur (build/test/run)" "scripts/install-deps.sh"
need_bin curl   "scripts/fetch-fonts.sh"        "scripts/install-deps.sh"
need_bin cc     "cargo build scripts"           "scripts/install-deps.sh"
need_bin size   "scripts/check-image-budgets.sh" "scripts/install-deps.sh  (binutils)"
need_bin pkg-config "cargo build scripts"       "scripts/install-deps.sh"
need_bin capnp  "the Cap'n Proto build scripts" "scripts/install-deps.sh  (capnproto)"
need_bin just   "every verification gate"       "scripts/install-deps.sh"
need_bin rg     "just deadcode / arch-gate / QEMU failure triage" "scripts/install-deps.sh  (ripgrep)"
need_bin dtc    "just check (board-goldens) and the board's FIT" "scripts/install-deps.sh  (device-tree-compiler / dtc)"
need_bin qemu-system-riscv64 "make run / just test-os / just start" "scripts/install-deps.sh"

if command -v python3 >/dev/null 2>&1; then
  # tools/systemui_profile_qemu_devices.py imports tomllib (3.11+).
  if python3 -c 'import sys, tomllib' >/dev/null 2>&1; then
    ok "python3 $(python3 -c 'import sys;print("%d.%d"%sys.version_info[:2])') with tomllib"
  else
    bad "python3 is older than 3.11 (no tomllib)" "install a newer python3"
  fi
else
  bad "python3 missing — needed by the QMP/proof tooling" "scripts/install-deps.sh"
fi

# --------- 3. rust toolchain -------------------------------------------------
head_ "Rust toolchain (pinned: $NIGHTLY)"
if ! command -v rustup >/dev/null 2>&1; then
  bad "rustup missing" "scripts/install-deps.sh"
elif ! command -v cargo >/dev/null 2>&1; then
  bad "cargo missing (rustup present)" "rustup toolchain install stable --profile minimal"
else
  if [ "$PATH_HAS_CARGO_BIN" = 1 ]; then
    ok "rustup + cargo on PATH"
  elif grep -qs 'cargo/env\|\.cargo/bin' "$HOME/.bashrc" "$HOME/.profile" "$HOME/.bash_profile" "$HOME/.zshrc"; then
    # rustup already wired the shell profile; only THIS shell predates it.
    soft "rustup is installed but ~/.cargo/bin is not on this shell's PATH" \
         "open a new terminal, or run: . \"\$HOME/.cargo/env\""
  else
    bad "rustup is installed but ~/.cargo/bin is on no PATH and no shell profile" \
        "add it: echo '. \"\$HOME/.cargo/env\"' >> ~/.bashrc && . \"\$HOME/.cargo/env\""
  fi

  installed_toolchains="$(rustup toolchain list 2>/dev/null)"
  case "$installed_toolchains" in
    *"$NIGHTLY"*) ok "toolchain $NIGHTLY installed" ;;
    *) bad "toolchain $NIGHTLY missing" "rustup toolchain install $NIGHTLY --profile minimal" ;;
  esac

  if rustup target list --installed --toolchain "$NIGHTLY" 2>/dev/null | grep -qx "$RV_TARGET"; then
    ok "target $RV_TARGET ($NIGHTLY)"
  else
    bad "target $RV_TARGET missing for $NIGHTLY" "rustup target add $RV_TARGET --toolchain $NIGHTLY"
  fi

  comps="$(rustup component list --installed --toolchain "$NIGHTLY" 2>/dev/null)"
  for c in clippy rustfmt rust-src llvm-tools miri; do
    if printf '%s' "$comps" | grep -q "^$c"; then
      ok "component $c"
    else
      case "$c" in
        # miri only gates `just test-all`; everything else gates `just check`.
        miri) soft "component miri missing — 'just test-all' cannot run miri-strict/miri-fs" \
                   "rustup component add miri --toolchain $NIGHTLY" ;;
        *) bad "component $c missing" "rustup component add $c --toolchain $NIGHTLY" ;;
      esac
    fi
  done

  # scripts/qemu-launcher.sh needs llvm-objcopy to turn the kernel ELF into a
  # flat image; it looks for it under the rustup toolchain tree.
  if compgen -G "$HOME/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-objcopy" >/dev/null \
     || command -v llvm-objcopy >/dev/null 2>&1; then
    ok "llvm-objcopy locatable"
  else
    bad "llvm-objcopy not found — scripts/qemu-launcher.sh cannot build the boot image" \
        "rustup component add llvm-tools-preview --toolchain $NIGHTLY"
  fi

  if command -v cargo-deny >/dev/null 2>&1; then
    ok "cargo-deny $(cargo deny --version 2>/dev/null | awk '{print $2}')"
  else
    bad "cargo-deny missing — 'just check' / 'just deny-check' cannot run" \
        "cargo +stable install --locked cargo-deny@0.18.9"
  fi

  if command -v cargo-nextest >/dev/null 2>&1; then
    ok "cargo-nextest"
  else
    soft "cargo-nextest missing — 'make test' falls back to plain 'cargo test'" \
         "cargo +stable install --locked cargo-nextest"
  fi
fi

# --------- 4. QEMU capabilities ---------------------------------------------
head_ "QEMU"
if command -v qemu-system-riscv64 >/dev/null 2>&1; then
  ok "qemu-system-riscv64 $(qemu-system-riscv64 --version 2>/dev/null | head -1 | awk '{print $4}')"

  if qemu-system-riscv64 -machine help 2>/dev/null | grep -q '^virt '; then
    ok "machine 'virt' supported"
  else
    bad "qemu-system-riscv64 does not know the 'virt' machine" "reinstall QEMU from your distro"
  fi

  # `just test-all` runs the SDHCI lane (`just ci-os-sdhci`, TASK-0246 P5): an
  # eMMC on QEMU's SD host behind the PCI host. Both are device models a QEMU
  # build may lack, and without them the lane's QEMU refuses to start.
  devices="$(qemu-system-riscv64 -device help 2>/dev/null)"
  if printf '%s\n' "$devices" | grep -q '^name "sdhci-pci"' \
     && printf '%s\n' "$devices" | grep -q '^name "emmc"'; then
    ok "devices 'sdhci-pci' + 'emmc' (the SDHCI lane, just ci-os-sdhci)"
  else
    bad "QEMU lacks the 'sdhci-pci' or 'emmc' device — 'just test-all' cannot run the SDHCI lane" \
        "a QEMU whose '-device help' lists both (the lane is proven on QEMU 11.1.1)"
  fi

  if [ "$WANT_GUI" != 0 ]; then
    # `just start` defaults to GPU_MODE=virgl in a `gtk,gl=on` window, and the
    # virgl proof lane uses egl-headless. On Debian/Ubuntu both live in
    # packages separate from the RISC-V emulator, so a QEMU that boots headless
    # can still be unable to open the window — this is the only honest test.
    displays="$(qemu-system-riscv64 -display help 2>/dev/null)"
    if printf '%s' "$displays" | grep -qw gtk; then
      ok "display backend 'gtk' (just start)"
    else
      bad "QEMU has no 'gtk' display backend — 'just start' cannot open a window" \
          "scripts/install-deps.sh   (Debian/Ubuntu: qemu-system-gui)"
    fi
    if printf '%s' "$displays" | grep -qw egl-headless; then
      ok "display backend 'egl-headless' (virgl proof lane, just start-vnc)"
    else
      bad "QEMU has no 'egl-headless' backend — the virgl path cannot come up" \
          "scripts/install-deps.sh   (Debian/Ubuntu: qemu-system-modules-opengl)"
    fi
  else
    ok "GUI checks skipped (GUI=0) — headless lanes only"
  fi
fi

# --------- 5. declared build inputs -----------------------------------------
head_ "Build inputs"
if scripts/fetch-inputs.sh --check >/dev/null 2>&1; then
  ok "submodules + pinned fonts present"
else
  bad "declared build inputs missing (submodules and/or pinned Noto fonts)" \
      "scripts/fetch-inputs.sh"
fi

# --------- 6. rootless podman ------------------------------------------------
head_ "Podman (container spur: 'make build' / 'make test')"
if [ "$WANT_PODMAN" = 0 ]; then
  ok "podman checks skipped (PODMAN=0) — use 'make build MODE=host'"
elif ! command -v podman >/dev/null 2>&1; then
  bad "podman missing — 'make build' (MODE=container, the default) cannot run" \
      "scripts/install-deps.sh, or use 'make build MODE=host'"
else
  if podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null | grep -q true; then
    ok "podman runs rootless"
  else
    bad "podman is not in rootless mode" \
        "install uidmap/shadow-utils, ensure /etc/subuid + /etc/subgid have an entry for $(id -un), then 'podman system migrate'"
  fi

  if grep -q "^$(id -un):" /etc/subuid 2>/dev/null && grep -q "^$(id -un):" /etc/subgid 2>/dev/null; then
    ok "/etc/subuid + /etc/subgid have an entry for $(id -un)"
  else
    bad "no /etc/subuid or /etc/subgid entry for $(id -un)" \
        "sudo usermod --add-subuids 100000-165535 --add-subgids 100000-165535 $(id -un) && podman system migrate"
  fi
fi

# --------- 7. board tools (TASK-0327) ----------------------------------------
# Capabilities again, not packages: the binaries `just board-*` execs, the udev
# rule that makes the board usable without sudo, and the serial group that owns
# the USB-UART adapter. The two "connected" lines are informational — a box
# without a board plugged in is not broken.
head_ "Board tools (just board-* — flash over USB, console over the debug UART)"
if [ "$WANT_BOARD" = 0 ]; then
  ok "board checks skipped (BOARD=0) — no reference board on this host"
else
  need_bin fastboot "just board-flash (boot-ROM download mode + U-Boot fastboot)" "scripts/install-deps.sh  (android-tools / fastboot)"
  need_bin adb      "just board-flash --verify (reads the eMMC back from the stock system)" "scripts/install-deps.sh  (android-tools / adb)"
  need_bin mkimage  "FIT images for the board boot chain (Block 1)" "scripts/install-deps.sh  (u-boot-tools / uboot-tools)"
  need_bin sgdisk   "verifying the flashed image's GPT" "scripts/install-deps.sh  (gdisk / gptfdisk)"
  serial_tool=""
  for t in picocom tio minicom; do
    if command -v "$t" >/dev/null 2>&1; then serial_tool="$t"; break; fi
  done
  if [ -n "$serial_tool" ]; then
    ok "serial terminal: $serial_tool (just board-serial)"
  else
    soft "no serial terminal (picocom/tio/minicom) — 'just board-serial' cannot open the debug UART" \
         "scripts/install-deps.sh  (picocom)"
  fi

  if scripts/install-board-access.sh --check >/dev/null 2>&1; then
    ok "udev rule + serial group set up (board reachable without sudo)"
  else
    # --check distinguishes "rule missing" from "group not effective yet";
    # surface its own wording rather than guessing here.
    detail="$(scripts/install-board-access.sh --check 2>&1 | sed -n 's/^.*\[warn\] //p' | head -1)"
    soft "board access incomplete: ${detail:-udev rule or serial group missing}" \
         "scripts/install-board-access.sh   (then log out and in)"
  fi

  if scripts/fetch-board-inputs.sh --check >/dev/null 2>&1; then
    ok "vendor boot pieces fetched and pinned (just board-flash)"
  else
    soft "vendor boot pieces not fetched — 'just board-flash' has nothing to stage" \
         "just board-inputs   (~250 MB download, once)"
  fi

  if command -v lsusb >/dev/null 2>&1; then
    boards="$(lsusb 2>/dev/null | grep -i ' 361c:' || true)"
    if [ -n "$boards" ]; then
      case "$boards" in
        *361c:1001*) ok "board connected in download/fastboot mode (361c:1001)" ;;
        *361c:0008*) ok "board connected, stock system gadget (361c:0008) — hold the download key at reset for fastboot" ;;
        *)           ok "board connected (vendor 361c): $(printf '%s' "$boards" | head -1 | sed 's/.*ID //')" ;;
      esac
    else
      [ "$QUIET" = 1 ] || printf '  \033[1;34m[info]\033[0m no board connected (vendor 361c not on the USB bus)\n'
    fi
  fi
  if compgen -G "/dev/serial/by-id/*" >/dev/null; then
    ok "serial adapter: $(ls /dev/serial/by-id/ | head -1)"
  else
    [ "$QUIET" = 1 ] || printf '  \033[1;34m[info]\033[0m no USB-UART adapter connected (/dev/serial/by-id empty) — needed for the console, not for flashing\n'
  fi
fi

# --------- verdict -----------------------------------------------------------
printf '\n'
if [ "$FAILURES" -eq 0 ]; then
  if [ "$WARNINGS" -gt 0 ]; then
    printf '\033[1;32m[doctor]\033[0m ready (%d warning(s) — optional pieces missing)\n' "$WARNINGS"
  else
    printf '\033[1;32m[doctor]\033[0m ready. Try: make build MODE=host  ·  just check  ·  just start\n'
  fi
  exit 0
fi

printf '\033[1;31m[doctor]\033[0m %d check(s) failed. Run: make initial-setup\n' "$FAILURES"
exit 1
