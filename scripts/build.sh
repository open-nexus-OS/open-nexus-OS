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
#     └── recipes/        <- (Optional) Your own extensions or modules
#
# 💡 This script:
#   1. Sets up basic environment variables
#   2. Runs 'make pull' in the Redox repo
#   3. Builds the selected config for the target architecture
#   4. (Optional) Triggers build logic in your own 'recipes/' directory
# =============================================================================

# Error Messages with row numbers
trap 'echo -e "\033[1;31m[CRASHED]\033[0m Fehler in Zeile $LINENO (Befehl: $BASH_COMMAND)"' ERR

# Color Output
INFO="\033[1;34m[INFO]\033[0m"
ERROR="\033[1;31m[ERROR]\033[0m"
SUCCESS="\033[1;32m[SUCCESS]\033[0m"

# Configuration
REDOX_DIR="redox"
ARCH="aarch64"
CONFIG="desktop-minimal.toml"

# Step 1: Show Environment
echo -e "$INFO Environment:"
echo "  ARCH=$ARCH"
echo "  CONFIG=$CONFIG"

# Step 2: Git update
cd "$REDOX_DIR" || { echo -e "$ERROR Redox directory not found!"; exit 1; }

# Step 3: Update repo and clean build artifacts
echo -e "$INFO Update Redox-Repository..."
make pull

# Step 3: Build
## Build Redox for $ARCH
echo -e "$INFO Build Redox for $ARCH with Configuration $CONFIG..."
./build.sh -f config/$ARCH/$CONFIG

## Build Nexus Integration
cd ..
echo -e "$INFO Build Nexus Integration..."

# Step 4: Show Build Status
echo -e "$SUCCESS open nexus OS build completed successfully!"