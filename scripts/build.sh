#!/bin/bash
set -e

# =============================================================================
# open nexus OS Build Script
#
# This script builds open nexus OS for a given architecture and configuration
# inside a Podman container. It expects that the Redox repository is
# already cloned and that the initial setup has been completed using
# 'initial_setup.sh'.
#
# 📁 Expected Structure:
#   ~/open-nexus-OS/
#     ├── redox/        <- Redox OS source code
#     ├── scripts/      <- This script lives here
#     ├── config/       <- Your custom configs (e.g. desktop.toml)
#     └── recipes/      <- (Optional) Your own extensions or modules
#
# 💡 This script:
#   1. Links your custom config into the redox/config/<arch>/ directory
#   2. Runs 'make pull' in the Redox repo
#   3. Builds the selected config for the target architecture
# =============================================================================

# Error Messages with row numbers
trap 'echo -e "\033[1;31m[CRASHED]\033[0m Fehler in Zeile $LINENO (Befehl: $BASH_COMMAND)"' ERR

# Color Output
INFO="\033[1;34m[INFO]\033[0m"
ERROR="\033[1;31m[ERROR]\033[0m"
SUCCESS="\033[1;32m[SUCCESS]\033[0m"

# Configuration
REDOX_DIR="redox"
ARCH="x86_64"  # Change this to your target architecture (e.g., x86_64, aarch64, riscv64gc)
CONFIG_NAME="desktop-nexus"
CONFIG_SOURCE="../../config/desktop.toml"
CONFIG_TARGET_PATH="config/$ARCH/$CONFIG_NAME.toml"

# Step 1: Show Environment
echo -e "$INFO Environment:"
echo "  ARCH=$ARCH"
echo "  CONFIG=$CONFIG"
echo -e "$INFO Using config: $CONFIG_PATH"

# Step 2: Git update
cd "$REDOX_DIR" || { echo -e "$ERROR Redox directory not found!"; exit 1; }

# Step 3: Link or copy your config into redox/config/<arch>/
mkdir -p "config/$ARCH"
ln -sf "$CONFIG_SOURCE" "$CONFIG_TARGET_PATH" || {
    echo -e "$ERROR Could not link config file $CONFIG_SOURCE to $CONFIG_TARGET_PATH"
    exit 1
}

# Step 4: Update repo and clean build artifacts
echo -e "$INFO Update Redox-Repository..."
make pull

# Step 5: Build with selected configuration
echo -e "$INFO Building Redox for $ARCH with configuration '$CONFIG_NAME'..."
#./build.sh -a "$ARCH" -c "$CONFIG_NAME"
./build.sh -f "$CONFIG_TARGET_PATH"

## Build Nexus Integration
cd ..
echo -e "$INFO Build Nexus Integration..."

# Step 4: Show Build Status
echo -e "$SUCCESS open nexus OS build completed successfully!"