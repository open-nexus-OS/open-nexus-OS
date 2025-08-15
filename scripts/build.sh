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
RECIPES_SOURCE="$(realpath recipes/gui)"
RECIPES_TARGET_PATH="cookbook/recipes/gui"
CONFIG_NAME="desktop"
CONFIG_SOURCE="$(realpath config/desktop.toml)"
CONFIG_TARGET_PATH="config/$ARCH/$CONFIG_NAME.toml"

# Step 1: Show Environment
echo -e "$INFO Environment:"
echo "  ARCH=$ARCH"
echo "  CONFIG=$CONFIG"
echo -e "$INFO Using config: $CONFIG_TARGET_PATH"
echo -e "$INFO Using recipes source: $RECIPES_SOURCE"
echo -e "$INFO Using recipes target: $RECIPES_TARGET_PATH"

# Step 2: Check if Redox directory exists and navigate to it
cd "$REDOX_DIR" || { 
    echo -e "$ERROR Redox directory not found!"
    exit 1 
}

# Step 3: Update repo and clean build artifacts
echo -e "$INFO Update Redox-Repository..."
make pull
#echo -e "$INFO Cleaning build artifacts..."
#make clean || {
#    echo -e "$ERROR Could not clean build artifacts. Continuing..."
#}

# Copy recipe files
echo -e "$INFO Copying recipe files..."
rsync -a "$RECIPES_SOURCE/" "$RECIPES_TARGET_PATH" || {
    echo -e "$ERROR Could not copy recipe files"
    exit 1
}

# Step 4: Copy your config into redox/config/<arch>/
echo -e "$INFO Using config source: $CONFIG_SOURCE"
echo -e "$INFO Using target: $PWD/$CONFIG_TARGET_PATH"

mkdir -p "config/$ARCH"
cp -f "$CONFIG_SOURCE" "$PWD/$CONFIG_TARGET_PATH" || {
    echo -e "$ERROR Could not copy config file $CONFIG_SOURCE to $PWD/$CONFIG_TARGET_PATH"
    exit 1
}

# Check source exists
if [[ ! -f "$CONFIG_TARGET_PATH" ]]; then
    echo "[ERROR] Config target not found: $CONFIG_TARGET_PATH"
    exit 1
fi

# Step 5: Build with selected configuration
echo -e "$INFO Building Redox for $ARCH with configuration '$CONFIG_NAME'..."
#./build.sh -a "$ARCH" -c "$CONFIG_NAME"
./build.sh -f "$CONFIG_TARGET_PATH"

## Build Nexus Integration
cd ..
echo -e "$INFO Build Nexus Integration..."

# Step 4: Show Build Status
echo -e "$SUCCESS open nexus OS build completed successfully!"