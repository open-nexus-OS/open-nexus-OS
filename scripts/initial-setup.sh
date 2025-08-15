#!/bin/bash
set -e

# =============================================================================
# Redox OS Initial Setup Script
#
# This script prepares your system to build Redox OS inside a Podman container.
#
# ❗ During the execution of podman_bootstrap.sh, you will be asked:
#   - Which QEMU version to install → Select: **qemu-full**
#   - Which container backend to use → Select: **crun**
#
# This script works on Arch/Manjaro, Ubuntu/Debian, and Fedora.
# It checks for required tools (curl, build tools, kernel headers),
# fetches the Podman bootstrap script from Redox upstream,
# and sets up your environment for development.
#
# ⚠️ It is recommended to clone the project inside your home directory (`cd ~`),
# to avoid permission issues during container execution.
# =============================================================================

# Error reporting
trap 'echo -e "\033[1;31m[CRASHED]\033[0m Error on line $LINENO (Command: $BASH_COMMAND)"' ERR

# Colored output
INFO="\033[1;34m[INFO]\033[0m"
ERROR="\033[1;31m[ERROR]\033[0m"
SUCCESS="\033[1;32m[SUCCESS]\033[0m"

# Detect OS
OS=""
if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$ID
fi

echo -e "$INFO Detected OS: $OS"

# Recommend cloning into home directory
if [[ "$PWD" != "$HOME"* ]]; then
    echo -e "$ERROR It is recommended to clone this repository into your home directory (e.g., 'cd ~ && git clone ...')"
    echo -e "$ERROR Current directory: $PWD"
    exit 1
fi

# Check for curl
if ! command -v curl &>/dev/null; then
    echo -e "$ERROR 'curl' is not installed."
    case "$OS" in
        arch|manjaro) echo "  sudo pacman -S curl" ;;
        ubuntu|debian) echo "  sudo apt install curl" ;;
        fedora) echo "  sudo dnf install curl" ;;
        *) echo "  Please install curl using your package manager." ;;
    esac
    exit 1
fi

# Check for base-devel or equivalent
check_build_tools() {
    case "$OS" in
        arch|manjaro)
            missing=$(comm -23 <(pacman -Sgq base-devel | sort) <(pacman -Qq | sort))
            if [[ -n "$missing" ]]; then
                echo "[ERROR] 'base-devel' is not fully installed. Missing:"
                echo "$missing"
                echo "Run:"
                echo "  sudo pacman -S --needed base-devel"
                exit 1
            fi
            ;;
        ubuntu|debian)
            if ! dpkg -s build-essential &>/dev/null; then
                echo -e "$ERROR 'build-essential' is not installed. Run:"
                echo "  sudo apt install build-essential"
                exit 1
            fi
            ;;
        fedora)
            if ! rpm -q @development-tools &>/dev/null; then
                echo -e "$ERROR Development tools are not fully installed. Run:"
                echo "  sudo dnf groupinstall \"Development Tools\""
                exit 1
            fi
            ;;
        *)
            echo -e "$ERROR Unknown OS. Please ensure required development tools are installed."
            exit 1
            ;;
    esac
}
check_build_tools

# Check for kernel headers
check_headers_installed() {
    case "$OS" in
        arch|manjaro)
            KVER=$(uname -r | cut -d '-' -f1 | cut -d '.' -f1,2 | tr -d '.')
            HEADER_PKG="linux${KVER}-headers"

            if ! pacman -Qs "^${HEADER_PKG}$" &>/dev/null; then
                echo -e "$ERROR Kernel headers not found. Run:"
                echo "  sudo pacman -S ${HEADER_PKG}"
                echo "Or install an LTS kernel with headers:"
                echo "  sudo mhwd-kernel -i linux65 && sudo reboot"
                exit 1
            fi
            ;;
        ubuntu|debian)
            if ! dpkg -l | grep linux-headers-$(uname -r) &>/dev/null; then
                echo -e "$ERROR Kernel headers not found. Run:"
                echo "  sudo apt install linux-headers-$(uname -r)"
                exit 1
            fi
            ;;
        fedora)
            if ! rpm -q kernel-devel &>/dev/null; then
                echo -e "$ERROR Kernel headers not found. Run:"
                echo "  sudo dnf install kernel-devel"
                exit 1
            fi
            ;;
        *)
            echo -e "$ERROR Unknown OS. Please install appropriate kernel headers manually."
            exit 1
            ;;
    esac
}
check_headers_installed

# Check if Redox directory exists and clone if not
echo -e "check if Redox directory exists..."
if [ ! -d "redox/.git" ]; then
  # Download and run podman_bootstrap.sh
  echo -e "$INFO Downloading podman_bootstrap.sh from Redox upstream..."
  curl -sf https://gitlab.redox-os.org/redox-os/redox/raw/master/podman_bootstrap.sh -o podman_bootstrap.sh

  echo -e "$INFO Executing podman_bootstrap.sh..."
  time bash -e podman_bootstrap.sh
else
  echo "Redox already exists, skipping clone."
fi



# Load Cargo environment
echo -e "$INFO Sourcing Cargo environment..."
source ~/.cargo/env

echo -e "$SUCCESS Initial Redox setup completed successfully!"
