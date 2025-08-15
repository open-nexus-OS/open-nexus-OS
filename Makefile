# ==============================================================================
# Makefile for Open-Nexus-OS Project
# ------------------------------------------------------------------------------
# This Makefile provides simple commands to run your build, clean, and utility
# scripts without manually typing the full "./scripts/script-name.sh" path.
#
# Usage:
#   make <target>
#
# Examples:
#   make initial-setup -> Sets up the initial environment
#   make clean	       -> This script cleans up the build environment
#   make build	       -> Builds Redox with the Nexus GUI package
#   make run	       -> Runs Redox OS with your current build
#
# Note:
#   - The SCRIPTS_DIR variable defines where your .sh files are stored.
#   - Each target calls its corresponding shell script.
# ==============================================================================

# Location of your shell scripts
SCRIPTS_DIR := scripts

# Default target when running 'make' without arguments
default:
	@echo "Available targets:"
	@echo "  make initial-setup - Sets up the initial environment"
	@echo "  make clean         - Clean the build environment"
	@echo "  make build         - Build Redox with Nexus package"
	@echo "  make run           - Run OS on qemu"

# ------------------------------------------------------------------------------

# Target: Initial Setup
# Description: Sets up the initial environment for the project.
initial-setup:
	bash $(SCRIPTS_DIR)/initial-setup.sh

# Target: Clean
# Description: Deletes all build artifacts, caches, and target directories.
clean:
	bash $(SCRIPTS_DIR)/clean.sh


# Target: Build Nexus
# Description: Compiles the Nexus GUI and its assets.
build:
	bash $(SCRIPTS_DIR)/build.sh

# Target: Run Nexus OS
# Description: Launches Nexus OS in the configured environment on qemu.
run:
	bash $(SCRIPTS_DIR)/run-qemu.sh $(filter-out $@,$(MAKECMDGOALS))
