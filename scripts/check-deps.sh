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
if [ -z "${HOME:-}" ]; then
  bad "\$HOME is not set" "run this as a normal login user"
else
  case "$REPO_ROOT/" in
    "$HOME"/*) ok "checkout is under \$HOME ($REPO_ROOT)" ;;
    *) bad "checkout is at $REPO_ROOT, which is NOT under \$HOME ($HOME)" \
           "move it (e.g. ~/open-nexus-OS) — rootless podman + cargo caches need user-owned paths" ;;
  esac
fi

owner="$(stat -c '%U' "$REPO_ROOT" 2>/dev/null || echo '?')"
if [ "$owner" = "$(id -un)" ]; then
  ok "checkout is owned by $(id -un)"
else
  bad "checkout is owned by '$owner', not $(id -un)" \
      "sudo chown -R $(id -un): $REPO_ROOT"
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
