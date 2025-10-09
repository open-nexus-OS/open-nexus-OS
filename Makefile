SHELL := /bin/sh
MODE ?= container
CONTAINER_TAG ?= open-nexus-os:dev
NIGHTLY ?= nightly-2025-01-15
CARGO_BIN ?= cargo
UID := $(shell id -u)
GID := $(shell id -g)
SELINUX_LABEL := $(shell command -v selinuxenabled >/dev/null 2>&1 && selinuxenabled && echo ":Z" || true)

.PHONY: initial-setup build test run pull clean

initial-setup:
	@echo "==> Checking podman rootless support"
	@scripts/check-rootless.sh
	@echo "==> Installing Rust targets"
	@rustup target add riscv64imac-unknown-none-elf
	@echo "==> Installing git hooks"
	@./scripts/fmt-clippy-deny.sh

build:
ifeq ($(MODE),container)
	@echo "==> Building workspace inside podman"
	@podman build -t $(CONTAINER_TAG) -f podman/Containerfile .
	@podman run --rm -t \
		--userns=keep-id -u $(UID):$(GID) \
		--entrypoint "" \
		-v "$(CURDIR)":/workspace$(SELINUX_LABEL) -w /workspace \
		-e CARGO_HOME=/workspace/.cargo \
		-e RUSTUP_HOME=/workspace/.rustup \
		-e CARGO_TARGET_DIR=/workspace/target \
		-e PATH=/workspace/.cargo/bin:/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
		$(CONTAINER_TAG) \
		sh -lc '\
		  echo "[1/2] host+os userspace build"; \
		  mkdir -p "$$RUSTUP_HOME" "$$CARGO_HOME"; \
		  rustup default stable; \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) build --workspace --exclude neuron --exclude neuron-boot --exclude samgrd --exclude bundlemgrd --exclude identityd --exclude dsoftbusd --exclude dist-data --exclude clipboardd --exclude notifd --exclude resmgrd --exclude searchd --exclude settingsd --exclude time-syncd && \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) build -p samgrd -p bundlemgrd -p identityd -p dsoftbusd -p dist-data -p clipboardd -p notifd -p resmgrd -p searchd -p settingsd -p time-syncd && \
		  echo "[2/2] cross build kernel (riscv)"; \
		  rustup toolchain list | grep -q "$(NIGHTLY)" || rustup toolchain install "$(NIGHTLY)" --profile minimal; \
		  rustup component add rust-src --toolchain "$(NIGHTLY)"; \
		  rustup target add riscv64imac-unknown-none-elf --toolchain "$(NIGHTLY)"; \
		                  $(CARGO_BIN) +$(NIGHTLY) build \
		                    --target riscv64imac-unknown-none-elf -p neuron-boot --release'
else
	@echo "==> Building workspace on host"
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"' cargo build --workspace --exclude neuron --exclude neuron-boot --exclude samgrd --exclude bundlemgrd --exclude identityd --exclude dsoftbusd --exclude dist-data --exclude clipboardd --exclude notifd --exclude resmgrd --exclude searchd --exclude settingsd --exclude time-syncd
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo build -p samgrd -p bundlemgrd -p identityd -p dsoftbusd -p dist-data -p clipboardd -p notifd -p resmgrd -p searchd -p settingsd -p time-syncd
endif

test:
ifeq ($(MODE),container)
	@echo "==> Running host-first tests inside podman"
	@podman run --rm -t \
		--userns=keep-id -u $(UID):$(GID) \
		--entrypoint "" \
		-v "$(CURDIR)":/workspace$(SELINUX_LABEL) -w /workspace \
		-e CARGO_HOME=/workspace/.cargo \
		-e RUSTUP_HOME=/workspace/.rustup \
		-e CARGO_TARGET_DIR=/workspace/target \
		-e PATH=/workspace/.cargo/bin:/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
 		$(CONTAINER_TAG) \
 		sh -lc '\
 		  echo "[tests] host-first only (exclude neuron)"; \
 		  $(CARGO_BIN) nextest run --workspace --exclude neuron'
else
	@echo "==> Running host-first tests"
	@cargo nextest run --workspace --exclude neuron
endif

run:
	@echo "==> Launching NEURON kernel under QEMU"
	@rustup toolchain list | grep -q "$(NIGHTLY)" || rustup toolchain install "$(NIGHTLY)" --profile minimal
	@rustup component add rust-src --toolchain "$(NIGHTLY)" >/dev/null 2>&1 || true
	@$(CARGO_BIN) +$(NIGHTLY) build --target riscv64imac-unknown-none-elf -p neuron-boot --release
	@RUN_TIMEOUT=$${RUN_TIMEOUT:-30s} ./scripts/run-qemu-rv64.sh

pull:
	@echo "==> Refreshing recipe sources"
	@find recipes -name recipe.toml -print | while read -r recipe; do \
		echo "Syncing $$recipe"; \
		grep '^\[source\]' -n "$$recipe" >/dev/null || true; \
	done

clean:
	@echo "==> Cleaning build artifacts"
	@cargo clean
	@rm -rf build
