#!/bin/bash
set -e

# =============================================================================
# Was experimental GUI Package Auto-Signer for Redox OS
#
# This script signs all packages in "open-nexus-os/recipes/gui" with a single PKGAR key.
# This prevents Redox from rebuilding them unnecessarily if they have not changed.
# It also ensures pkgar and pkgar-keys are installed and generates keys if missing.
#
# Usage:
#   ./sign-nexus.sh
#
# Currently not in use in the build process and not updated, but can be integrated 
# after building GUI packages. So don't use if you don't know what you're doing.
# =============================================================================

# Config
PROJECT_DIR="$(pwd)"
RECIPES_DIR="$PROJECT_DIR/recipes/gui"
PRIVATE_KEY="$PROJECT_DIR/my-nexus-key.pkgar"
PUBLIC_KEY="$PROJECT_DIR/my-nexus-key.pub"
PKGAR_BIN="$(which pkgar || true)"
PKGAR_KEYS_BIN="$(which pkgar-keys || true)"
KEYS_TOML="$PROJECT_DIR/cookbook/redoxer/etc/pkgar/keys.toml"
ARCH="x86_64"

# Colors
INFO="\033[1;34m[INFO]\033[0m"
OK="\033[1;32m[OK]\033[0m"
ERROR="\033[1;31m[ERROR]\033[0m"

# ----------------------------
# 1. Check if pkgar is installed
# ----------------------------
if [ -z "$PKGAR_BIN" ]; then
    echo -e "$INFO pkgar not found, building..."
    cargo install --locked pkgar
    PKGAR_BIN="$(which pkgar)"
else
    echo -e "$INFO pkgar is present."
fi

# ----------------------------
# 2. Check if pkgar-keys is installed
# ----------------------------
if [ -z "$PKGAR_KEYS_BIN" ]; then
    echo -e "$INFO pkgar-keys not found, building..."
    cargo install --locked pkgar-keys
    PKGAR_KEYS_BIN="$(which pkgar-keys)"
else
    echo -e "$INFO pkgar-keys is present."
fi

# ----------------------------
# 3. Generate PKGAR key pair if missing
# ----------------------------
if [ ! -f "$PRIVATE_KEY" ] || [ ! -f "$PUBLIC_KEY" ]; then
    echo -e "$INFO Creating PKGAR key pair..."
    mkdir -p "$(dirname "$PRIVATE_KEY")"
    "$PKGAR_KEYS_BIN" gen --out-private "$PRIVATE_KEY" --out-public "$PUBLIC_KEY" --no-passphrase
    echo -e "$OK Keys created: $PRIVATE_KEY / $PUBLIC_KEY"
else
    echo -e "$INFO PKGAR keys already exist."
fi

# ----------------------------
# 4. Add public key to trusted keys
# ----------------------------
mkdir -p "$(dirname "$KEYS_TOML")"
if ! grep -q "$PUBLIC_KEY" "$KEYS_TOML" 2>/dev/null; then
    echo -e "$INFO Adding public key to trusted keys..."
    echo -e "\n[[keys]]\npath = \"$PUBLIC_KEY\"" >> "$KEYS_TOML"
    echo -e "$OK Public key added."
else
    echo -e "$INFO Public key already trusted."
fi

# ----------------------------
# 5. Sign all GUI packages
# ----------------------------
echo -e "$INFO Signing all GUI packages..."
for dir in "$RECIPES_DIR"/*; do
    if [ -d "$dir" ]; then
        STAGE_DIR="$PROJECT_DIR/cookbook/recipes/gui/$(basename "$dir")/target/${ARCH}-unknown-redox/stage"
        if [ -d "$STAGE_DIR" ]; then
            echo -e "$INFO Signing package: $(basename "$dir")"
            "$PKGAR_BIN" sign --key "$PRIVATE_KEY" "$STAGE_DIR"
            echo -e "$OK Signed: $(basename "$dir")"
        else
            echo -e "$INFO Stage directory not found for package: $(basename "$dir"), skipping."
        fi
    fi
done

echo -e "$OK All GUI packages signed successfully."
