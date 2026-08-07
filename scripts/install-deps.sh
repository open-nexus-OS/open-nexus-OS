#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Install the host-side build/QEMU/security tooling required by the
#          Open Nexus OS Makefile and `just` recipes, on Debian/Ubuntu, Fedora
#          and Arch. Package NAMES drift between distro releases (Ubuntu 26.04
#          split RISC-V out of qemu-system-misc and left `qemu-system-riscv64`
#          as an uninstallable virtual name), so every entry is a CANDIDATE
#          LIST and the first name the local package index actually knows wins.
#          What a working host really needs is verified separately and
#          distro-agnostically by scripts/check-deps.sh.
# OWNERS:  @tools-team
# STATUS:  Functional
# API_STABILITY: Stable
# TEST_COVERAGE: scripts/check-deps.sh (post-condition); --print-packages for
#          the Fedora/Arch tables on any host
# DEPENDS_ON: a supported package manager (apt, dnf/yum, pacman)
#
# Usage:
#   scripts/install-deps.sh              # detect distro, prompt before sudo
#   scripts/install-deps.sh --yes        # non-interactive (CI / scripted setup)
#   scripts/install-deps.sh --check      # only report what's missing, no install
#   scripts/install-deps.sh --no-gui     # skip GTK/EGL/virgl (headless hosts)
#   scripts/install-deps.sh --print-packages   # resolve + print, install nothing
#
# Env:
#   NEXUS_FORCE_FAMILY=debian|fedora|arch   override distro detection (testing)
#
# Exit codes:
#   0  all required packages installed (or installed successfully)
#   1  unsupported distro / unknown package manager / unresolvable core package
#   2  install failed
#   3  --check ran and packages are missing

set -euo pipefail

NIGHTLY="nightly-2025-01-15"     # keep in sync with rust-toolchain.toml
RV_TARGET="riscv64imac-unknown-none-elf"
CARGO_DENY_VERSION="0.18.9"      # keep in sync with podman/Containerfile + ci.yml

ASSUME_YES=0
CHECK_ONLY=0
PRINT_ONLY=0
WANT_GUI=1

for arg in "$@"; do
  case "$arg" in
    --yes|-y)         ASSUME_YES=1 ;;
    --check)          CHECK_ONLY=1 ;;
    --no-gui)         WANT_GUI=0 ;;
    --print-packages) PRINT_ONLY=1 ;;
    -h|--help)
      sed -n '18,32p' "$0"
      exit 0
      ;;
    *) echo "[error] unknown flag: $arg" >&2; exit 1 ;;
  esac
done

log()  { printf '\033[1;34m[deps]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[deps][warn]\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31m[deps][error]\033[0m %s\n' "$*" >&2; }

# --------- detect distro ----------------------------------------------------
DISTRO=""
DISTRO_LIKE=""
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO="${ID:-}"
  DISTRO_LIKE="${ID_LIKE:-}"
fi

family=""
case "$DISTRO" in
  debian|ubuntu|linuxmint|pop|raspbian) family="debian" ;;
  fedora|rhel|centos|rocky|almalinux)   family="fedora" ;;
  arch|manjaro|endeavouros|cachyos)     family="arch"   ;;
  *)
    case " $DISTRO_LIKE " in
      *debian*) family="debian" ;;
      *fedora*|*rhel*) family="fedora" ;;
      *arch*) family="arch" ;;
    esac
    ;;
esac

if [ -n "${NEXUS_FORCE_FAMILY:-}" ]; then
  case "$NEXUS_FORCE_FAMILY" in
    debian|fedora|arch) family="$NEXUS_FORCE_FAMILY" ;;
    *) err "NEXUS_FORCE_FAMILY must be debian|fedora|arch"; exit 1 ;;
  esac
  warn "distro family forced to '$family' (NEXUS_FORCE_FAMILY)"
fi

if [ -z "$family" ]; then
  err "unsupported distro (ID=$DISTRO ID_LIKE=$DISTRO_LIKE)."
  err "Supported families: debian, fedora, arch. Install equivalents of:"
  err "  a C toolchain, pkg-config, git, curl, python3 (>=3.11), capnproto,"
  err "  ripgrep, just, qemu-system-riscv64 (+ its GTK/OpenGL display modules),"
  err "  podman (rootless), openssl headers, rustup"
  err "Then run scripts/check-deps.sh to see what is still missing."
  exit 1
fi

log "detected distro family: $family (ID=${DISTRO:-unknown})"

# --------- package tables ---------------------------------------------------
# One entry per line, "a|b" = candidate list, first name the local index knows
# wins. CORE is required; GUI and OPTIONAL only warn when unavailable so a
# renamed GUI package can never block the whole setup.
#
# CORE deliberately omits `just`, `ripgrep`, `cargo-deny` and `cargo-nextest`
# where a distro may not ship them — ensure_cargo_tool() below installs any of
# those from crates.io as the universal fallback.
case "$family" in
  debian)
    CORE=(
      ca-certificates curl git
      build-essential pkg-config mold
      python3 python3-venv python3-pip
      capnproto just ripgrep
      libssl-dev
      "qemu-system-riscv|qemu-system-misc"
      podman uidmap "passt|slirp4netns" fuse-overlayfs
    )
    # `just start` runs virgl in a `gtk,gl=on` window; on Debian/Ubuntu the GTK
    # and OpenGL display backends are separate packages that
    # --no-install-recommends would otherwise leave out.
    GUI=(qemu-system-gui qemu-system-modules-opengl)
    OPTIONAL=(gdb meson ninja-build flatbuffers-compiler)
    ;;
  fedora)
    CORE=(
      ca-certificates curl git
      gcc gcc-c++ make binutils "pkgconf-pkg-config|pkgconfig" mold
      python3 python3-pip
      capnproto just ripgrep
      openssl-devel
      "qemu-system-riscv-core|qemu-system-riscv"
      podman shadow-utils "passt|slirp4netns" fuse-overlayfs
    )
    GUI=(qemu-ui-gtk qemu-ui-opengl qemu-ui-egl-headless
         qemu-device-display-virtio-gpu-gl qemu-device-display-virtio-gpu-pci-gl
         virglrenderer mesa-dri-drivers)
    OPTIONAL=(gdb meson ninja-build "flatbuffers-compiler|flatbuffers-devel")
    ;;
  arch)
    CORE=(
      ca-certificates curl git
      base-devel pkgconf mold
      python python-pip
      capnproto just ripgrep
      openssl
      qemu-system-riscv
      podman shadow "passt|slirp4netns" fuse-overlayfs
    )
    GUI=(qemu-ui-gtk qemu-ui-opengl qemu-ui-egl-headless
         qemu-hw-display-virtio-gpu-gl qemu-hw-display-virtio-gpu-pci-gl
         virglrenderer mesa)
    OPTIONAL=(gdb meson ninja flatbuffers)
    ;;
esac

# --------- package-manager adapters -----------------------------------------
# pkg_available: 0 = installable package exists, 1 = does not, 2 = cannot tell
# (package manager absent — happens under NEXUS_FORCE_FAMILY on a foreign host).
PM=""
case "$family" in
  debian)
    # `apt-cache show` prints records only for REAL packages: a purely virtual
    # name like qemu-system-riscv64 prints nothing. That is exactly the
    # distinction we need, and unlike `apt-cache policy` it is locale-independent.
    pkg_available() {
      command -v apt-cache >/dev/null 2>&1 || return 2
      apt-cache show -- "$1" 2>/dev/null | grep -q '^Package:'
    }
    is_installed() {
      dpkg-query -W -f='${Status}' -- "$1" 2>/dev/null | grep -q 'ok installed'
    }
    install_cmd() {
      sudo apt-get update
      if [ "$ASSUME_YES" = 1 ]; then
        sudo apt-get install -y --no-install-recommends "$@"
      else
        sudo apt-get install --no-install-recommends "$@"
      fi
    }
    ;;
  fedora)
    PM=$(command -v dnf 2>/dev/null || command -v yum 2>/dev/null || true)
    pkg_available() {
      [ -n "$PM" ] || return 2
      "$PM" --quiet info -- "$1" >/dev/null 2>&1
    }
    is_installed() { rpm -q -- "$1" >/dev/null 2>&1; }
    install_cmd() {
      if [ "$ASSUME_YES" = 1 ]; then sudo "$PM" install -y "$@"; else sudo "$PM" install "$@"; fi
    }
    ;;
  arch)
    pkg_available() {
      command -v pacman >/dev/null 2>&1 || return 2
      pacman -Si -- "$1" >/dev/null 2>&1
    }
    is_installed() { pacman -Qq -- "$1" >/dev/null 2>&1; }
    install_cmd() {
      if [ "$ASSUME_YES" = 1 ]; then sudo pacman -S --needed --noconfirm "$@"; else sudo pacman -S --needed "$@"; fi
    }
    ;;
esac

# resolve_pkg "a|b|c" -> sets RESOLVED_NAME + RESOLVE_STATE (ok|guess|none),
# returns non-zero when nothing matched. Deliberately NOT via command
# substitution: a subshell would drop RESOLVE_STATE.
RESOLVED_NAME=""
RESOLVE_STATE=""
resolve_pkg() {
  local candidates first c rc
  IFS='|' read -r -a candidates <<<"$1"
  first="${candidates[0]}"
  # An installed package needs no index entry, and asking dpkg/rpm/pacman is
  # both faster and immune to a package index that is momentarily unreadable —
  # `apt-get update` (apt-daily, unattended-upgrades) rewrites the binary cache
  # in place, and a probe that lands in that window sees a package vanish. On a
  # re-run this is also the common case.
  for c in "${candidates[@]}"; do
    if is_installed "$c" 2>/dev/null; then
      RESOLVED_NAME="$c"; RESOLVE_STATE="ok"; return 0
    fi
  done
  for c in "${candidates[@]}"; do
    if pkg_available "$c"; then
      RESOLVED_NAME="$c"; RESOLVE_STATE="ok"; return 0
    else
      rc=$?
      if [ "$rc" -eq 2 ]; then
        RESOLVED_NAME="$first"; RESOLVE_STATE="guess"; return 0
      fi
    fi
  done
  RESOLVED_NAME="$first"; RESOLVE_STATE="none"; return 1
}

resolved_core=()
resolved_extra=()
unresolved_core=()
unresolved_extra=()
guessed=()

resolve_group() {
  local kind=$1; shift
  local entry
  for entry in "$@"; do
    if resolve_pkg "$entry"; then
      [ "$RESOLVE_STATE" = "guess" ] && guessed+=("$RESOLVED_NAME")
      if [ "$kind" = core ]; then
        resolved_core+=("$RESOLVED_NAME")
      else
        resolved_extra+=("$RESOLVED_NAME")
      fi
    else
      if [ "$kind" = core ]; then unresolved_core+=("$entry"); else unresolved_extra+=("$entry"); fi
    fi
  done
}

resolve_group core "${CORE[@]}"
if [ "$WANT_GUI" = 1 ]; then
  resolve_group extra "${GUI[@]}"
else
  log "GUI packages skipped (--no-gui): 'just start' will only work headless."
fi
resolve_group extra "${OPTIONAL[@]}"

for entry in "${unresolved_extra[@]+"${unresolved_extra[@]}"}"; do
  warn "no package found for '${entry//|/ or }' — skipping (non-essential)"
done

if [ "${#guessed[@]}" -gt 0 ]; then
  warn "package index unavailable; assuming first candidate for: ${guessed[*]}"
fi

if [ "${#unresolved_core[@]}" -gt 0 ]; then
  err "required package(s) not found in the $family package index:"
  for entry in "${unresolved_core[@]}"; do
    err "  ${entry//|/ or }"
  done
  err "If these names look ordinary, the package index was momentarily"
  err "unreadable (apt-get update / unattended-upgrades rewriting it) — just"
  err "re-run. Otherwise enable the missing repository or install an"
  err "equivalent by hand; scripts/check-deps.sh shows what is still missing."
  exit 1
fi

PKGS=("${resolved_core[@]}" "${resolved_extra[@]+"${resolved_extra[@]}"}")

if [ "$PRINT_ONLY" = 1 ]; then
  printf '%s\n' "${PKGS[@]}"
  exit 0
fi

# --------- detect what's missing -------------------------------------------
missing=()
for pkg in "${PKGS[@]}"; do
  is_installed "$pkg" || missing+=("$pkg")
done

if [ "${#missing[@]}" -eq 0 ]; then
  log "all required packages already installed (${#PKGS[@]} checked)."
else
  log "missing ${#missing[@]} package(s): ${missing[*]}"

  if [ "$CHECK_ONLY" = 1 ]; then
    warn "--check mode: not installing. Re-run without --check to install."
    exit 3
  fi

  if [ "$ASSUME_YES" != 1 ] && [ -t 0 ]; then
    printf '[deps] About to install with sudo. Continue? [y/N] '
    read -r reply
    case "$reply" in
      y|Y|yes|YES) ;;
      *) err "aborted by user."; exit 2 ;;
    esac
  fi

  # Authenticate once, up front. Without this the failure surfaces as two
  # opaque "sudo: a terminal is required" lines from inside install_cmd, which
  # reads as a broken script rather than "you have no TTY". Common in
  # non-interactive contexts: editor/agent-run shells, CI without a pty.
  if [ "$(id -u)" -ne 0 ]; then
    if ! sudo -n true 2>/dev/null; then
      if [ -t 0 ]; then
        sudo -v || { err "sudo authentication failed."; exit 2; }
      else
        err "sudo needs a password but this shell has no terminal."
        err "Run this script from a normal interactive terminal:"
        err "    cd $(pwd) && ./scripts/install-deps.sh --yes"
        err "(or pre-authenticate there with 'sudo -v', then re-run here)"
        exit 2
      fi
    fi
  fi

  if ! install_cmd "${missing[@]}"; then
    err "package install failed."
    exit 2
  fi

  log "package install completed."
fi

[ "$CHECK_ONLY" = 1 ] && exit 0

# --------- post-install: rustup bootstrap -----------------------------------
# Distro `rustup` packages are just the manager; they still need toolchains.
# Where no rustup package exists, fall back to the upstream installer — the
# same one podman/Containerfile uses, so host and container agree.
ORIG_PATH="$PATH"
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v rustup >/dev/null 2>&1; then
  log "rustup not on PATH; bootstrapping from https://sh.rustup.rs"
  # No --no-modify-path: on a fresh box nothing else puts ~/.cargo/bin on PATH,
  # and a rustup the user's next shell cannot see is worse than useless.
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal
fi

if ! command -v rustup >/dev/null 2>&1; then
  err "rustup still unavailable after bootstrap."
  exit 2
fi

# stable is needed regardless of what the user defaults to: cargo-deny requires
# rustc >= 1.88 and the project pins nightly-2025-01-15 (rustc 1.86).
log "installing stable toolchain (host builds + cargo-installed tools)"
rustup toolchain install stable --profile minimal
if ! rustup show active-toolchain >/dev/null 2>&1; then
  rustup default stable
fi

log "installing pinned toolchain $NIGHTLY + components"
rustup toolchain install "$NIGHTLY" --profile minimal
# clippy/rustfmt/rust-src/llvm-tools-preview mirror rust-toolchain.toml; miri is
# extra and required by `just test-all` (miri-strict / miri-fs). llvm-tools
# supplies the llvm-objcopy that scripts/qemu-launcher.sh looks for.
rustup component add clippy rustfmt rust-src llvm-tools-preview miri --toolchain "$NIGHTLY"

log "installing RISC-V cross-compilation target ($RV_TARGET)"
rustup target add "$RV_TARGET" --toolchain "$NIGHTLY"
# The Makefile's container spur builds under `rustup default stable`, so the
# default toolchain needs the target too.
rustup target add "$RV_TARGET" || warn "could not add $RV_TARGET to the default toolchain"

# --------- post-install: cargo-installed tools ------------------------------
# Universal fallback for anything the distro did not ship. `cargo install`
# compiles, so only ever run when the binary is genuinely absent.
ensure_cargo_tool() {
  local bin=$1 spec=$2
  if command -v "$bin" >/dev/null 2>&1; then
    log "$bin present ($(command -v "$bin"))"
    return 0
  fi
  log "installing $spec from crates.io (compiles, this takes a few minutes)"
  # cargo-deny needs rustc >= 1.88 while the pinned nightly is 1.86, so these
  # build on stable, never on the pinned nightly.
  if ! cargo +stable install --locked "$spec"; then
    warn "cargo install $spec failed — install $bin by hand"
    return 1
  fi
}

ensure_cargo_tool just just || true
ensure_cargo_tool rg ripgrep || true
ensure_cargo_tool cargo-deny "cargo-deny@${CARGO_DENY_VERSION}" || true
ensure_cargo_tool cargo-nextest cargo-nextest || true

case ":$ORIG_PATH:" in
  *":$HOME/.cargo/bin:"*) ;;
  *) warn "~/.cargo/bin is not on your PATH — add '. \"\$HOME/.cargo/env\"' to ~/.bashrc and open a new shell" ;;
esac

# A bare distro install has no `make` either (it arrives with build-essential /
# base-devel), so this script is runnable standalone before `make` exists — and
# `make initial-setup` is idempotent, so running it afterwards costs nothing.
log "package + toolchain setup complete."
