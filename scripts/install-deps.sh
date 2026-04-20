#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Install the host-side build/QEMU/security tooling required by the
#          Open Nexus OS Makefile and `just` recipes. Mirrors the package
#          set baked into podman/Containerfile so the host spur and the
#          container spur stay byte-for-byte equivalent.
# OWNERS:  @tools-team
# STATUS:  Functional
# DEPENDS_ON: a supported package manager (apt, dnf/yum, pacman)
#
# Usage:
#   scripts/install-deps.sh           # detect distro, prompt before sudo
#   scripts/install-deps.sh --yes     # non-interactive (CI / scripted setup)
#   scripts/install-deps.sh --check   # only report what's missing, no install
#
# Exit codes:
#   0  all required packages installed (or installed successfully)
#   1  unsupported distro / unknown package manager
#   2  install failed
#   3  --check ran and packages are missing

set -euo pipefail

ASSUME_YES=0
CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --yes|-y) ASSUME_YES=1 ;;
    --check)  CHECK_ONLY=1 ;;
    -h|--help)
      sed -n '4,18p' "$0"
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

if [ -z "$family" ]; then
  err "unsupported distro (ID=$DISTRO ID_LIKE=$DISTRO_LIKE)."
  err "Supported families: debian, fedora, arch. Install equivalents of:"
  err "  build-essential pkg-config git python3 python3-pip meson ninja"
  err "  capnproto flatbuffers-compiler qemu-system-riscv64 gdb mold libssl-dev"
  err "  podman rustup curl ca-certificates"
  exit 1
fi

log "detected distro family: $family (ID=$DISTRO)"

# --------- package mapping --------------------------------------------------
# Mirror podman/Containerfile + add what the host spur needs that the container
# already has baked in (rustup, podman). curl + ca-certificates are bootstrap
# requirements for rustup itself.
case "$family" in
  debian)
    PKGS=(
      ca-certificates curl git
      build-essential pkg-config mold
      python3 python3-venv python3-pip
      meson ninja-build
      capnproto flatbuffers-compiler
      qemu-system-misc qemu-system-riscv64
      gdb libssl-dev
      podman rustup
    )
    ;;
  fedora)
    PKGS=(
      ca-certificates curl git
      gcc gcc-c++ make pkgconf-pkg-config mold
      python3 python3-pip
      meson ninja-build
      capnproto flatbuffers-compiler
      qemu-system-riscv-core
      gdb openssl-devel
      podman rustup
    )
    ;;
  arch)
    PKGS=(
      ca-certificates curl git
      base-devel pkgconf mold
      python python-pip
      meson ninja
      capnproto flatbuffers
      qemu-system-riscv
      gdb openssl
      podman rustup
    )
    ;;
esac

# --------- detect what's missing -------------------------------------------
case "$family" in
  debian)
    is_installed() { dpkg -s "$1" >/dev/null 2>&1; }
    install_cmd() {
      if [ "$ASSUME_YES" = 1 ]; then
        sudo apt-get update && sudo apt-get install -y --no-install-recommends "$@"
      else
        sudo apt-get update && sudo apt-get install --no-install-recommends "$@"
      fi
    }
    ;;
  fedora)
    PM=$(command -v dnf || command -v yum)
    is_installed() { rpm -q "$1" >/dev/null 2>&1; }
    install_cmd() {
      if [ "$ASSUME_YES" = 1 ]; then
        sudo "$PM" install -y "$@"
      else
        sudo "$PM" install "$@"
      fi
    }
    ;;
  arch)
    is_installed() { pacman -Qq "$1" >/dev/null 2>&1; }
    install_cmd() {
      if [ "$ASSUME_YES" = 1 ]; then
        sudo pacman -S --needed --noconfirm "$@"
      else
        sudo pacman -S --needed "$@"
      fi
    }
    ;;
esac

missing=()
for pkg in "${PKGS[@]}"; do
  if is_installed "$pkg"; then
    continue
  fi
  missing+=("$pkg")
done

if [ "${#missing[@]}" -eq 0 ]; then
  log "all required packages already installed (${#PKGS[@]} checked)."
  exit 0
fi

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

if ! install_cmd "${missing[@]}"; then
  err "package install failed."
  exit 2
fi

log "package install completed."

# --------- post-install: rustup bootstrap if absent -------------------------
# `rustup` package on Fedora/Arch is just the manager; it still needs an
# initial toolchain. On Debian/Ubuntu the rustup package is also a thin
# bootstrap. Run `rustup-init` non-interactively only if no toolchain exists.
if command -v rustup >/dev/null 2>&1; then
  if ! rustup show active-toolchain >/dev/null 2>&1; then
    log "no active rust toolchain; installing nightly-2025-01-15 (project-pinned)"
    rustup toolchain install nightly-2025-01-15 --profile minimal
    rustup default nightly-2025-01-15
  fi
fi

log "done. Next: 'make initial-setup' (continues with QEMU patch + git hooks) or 'make build'."
