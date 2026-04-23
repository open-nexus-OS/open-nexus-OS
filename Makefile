SHELL := /bin/sh
MODE ?= container
CONTAINER_TAG ?= open-nexus-os:dev
NIGHTLY ?= nightly-2025-01-15
CARGO_BIN ?= cargo
SMP ?= 2
HOST_RUSTFLAGS := --check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"

# Canonical artifact paths that `make build` must produce and that
# `make test` / `make run` consume via NEXUS_SKIP_BUILD=1
# (see scripts/run-qemu-rv64.sh).
RV_TARGET := riscv64imac-unknown-none-elf
TARGET_DIR := target
INIT_ELF := $(TARGET_DIR)/$(RV_TARGET)/release/init-lite
KERNEL_ELF := $(TARGET_DIR)/$(RV_TARGET)/release/neuron-boot
UID := $(shell id -u)
GID := $(shell id -g)
SELINUX_LABEL := $(shell command -v selinuxenabled >/dev/null 2>&1 && selinuxenabled && echo ":Z" || true)

.PHONY: initial-setup build test run pull clean
.PHONY: run-init-host test-init-host
.PHONY: dep-gate

initial-setup:
	@echo "==> Checking workspace location (must be under \$$HOME for rootless podman + cargo cache permissions)"
	@case "$(CURDIR)" in \
	  $$HOME/*) echo "[ok] workspace under \$$HOME ($(CURDIR))" ;; \
	  *) echo "[error] workspace is at $(CURDIR) which is NOT under \$$HOME ($$HOME)."; \
	     echo "[error] rootless podman + cargo target/ caches need user-owned paths."; \
	     echo "[error] move the checkout under \$$HOME (e.g. ~/open-nexus-OS) and re-run."; \
	     exit 1 ;; \
	esac
	@echo "==> Installing host packages (mirrors podman/Containerfile)"
	@./scripts/install-deps.sh $(if $(YES),--yes,)
	@echo "==> Checking podman rootless support"
	@scripts/check-rootless.sh
	@echo "==> Checking QEMU build deps (for virtio-mmio modern patch)"
	@command -v ninja >/dev/null 2>&1 || echo "[warn] ninja not found (required for QEMU build)"
	@command -v meson >/dev/null 2>&1 || echo "[warn] meson not found (required for QEMU build)"
	@echo "==> Installing Rust targets"
	@rustup target add riscv64imac-unknown-none-elf
	@echo "==> Running pre-commit gate (fmt + clippy + cargo-deny)"
	@# Note: this does NOT install a git hook. It just runs the same gate
	@# that a pre-commit hook would. To wire it as an actual hook, do:
	@#   ln -sf ../../scripts/fmt-clippy-deny.sh .git/hooks/pre-commit
	@./scripts/fmt-clippy-deny.sh
	@echo "==> QEMU modern virtio-mmio patch: run ./tools/qemu/build-modern.sh"

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
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) build --workspace --exclude neuron --exclude neuron-boot --exclude samgrd --exclude bundlemgrd --exclude identityd --exclude dsoftbusd --exclude dist-data --exclude clipboardd --exclude notifd --exclude resmgrd --exclude searchd --exclude settingsd --exclude time-syncd --exclude netstackd && \
		  echo "[1a/2] pre-compile host test binaries (consumed by make test)"; \
		  if $(CARGO_BIN) nextest --version >/dev/null 2>&1; then \
		    RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) nextest list --workspace --exclude neuron --exclude neuron-boot >/dev/null; \
		  else \
		    RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) test --workspace --exclude neuron --exclude neuron-boot --no-run; \
		  fi && \
		  echo "[1b/2] cross-compile OS services (riscv64, --release) — full DEFAULT_SERVICE_LIST"; \
		  # CRITICAL: services MUST be --release so they land under \
		  #   target/riscv64imac-unknown-none-elf/release/<svc> \
		  # which matches the INIT_LITE_SERVICE_<NAME>_ELF paths set in step \
		  # [1d/2] below and the paths qemu-test.sh / run-qemu-rv64.sh expect. \
		  # Without --release init-lite build.rs `std::fs::copy` fails in \
		  # fresh checkouts (CI) with "No such file or directory" on the \
		  # release path. Locally it can mask itself when prior `just test-os` \
		  # runs already populated `release/`. \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build \
		    -p keystored -p rngd -p policyd -p logd -p metricsd \
		    -p samgrd -p bundlemgrd -p statefsd -p updated -p timed \
		    -p packagefsd -p vfsd -p execd -p netstackd -p dsoftbusd \
		    -p selftest-client \
		    --target riscv64imac-unknown-none-elf --no-default-features --features os-lite --release && \
		  echo "[1c/2] RFC-0009 dep-gate (OS graph)"; \
		  forbidden="parking_lot parking_lot_core getrandom"; \
		  services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd timed metricsd"; \
		  found=0; \
		  for svc in $$services; do \
		    tree_output=$$($(CARGO_BIN) +$(NIGHTLY) tree -p "$$svc" --target riscv64imac-unknown-none-elf --no-default-features --features os-lite 2>&1 || true); \
		    for f in $$forbidden; do \
		      echo "$$tree_output" | grep -qE "^[│├└ ]*$$f " && echo "[FAIL] $$svc pulled forbidden crate $$f" && found=1; \
		    done; \
		  done; \
		  test "$$found" -eq 0 && echo "[PASS] RFC-0009 dep-gate" || (echo "[FAIL] RFC-0009 dep-gate" && exit 1); \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p nexus-init --lib --target riscv64imac-unknown-none-elf --no-default-features --features os-lite && \
                  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p nexus-log --features sink-userspace --target riscv64imac-unknown-none-elf --release && \
		  echo "[1d/2] build init-lite with INIT_LITE_SERVICE_*_ELF env vars (bakes service ELFs into init-lite)"; \
		  svc_env=""; \
		  for svc in keystored rngd policyd logd metricsd samgrd bundlemgrd statefsd updated timed packagefsd vfsd execd netstackd dsoftbusd selftest-client; do \
		    upper=$$(echo "$$svc" | tr '[:lower:]' '[:upper:]' | tr '-' '_'); \
		    svc_env="$$svc_env INIT_LITE_SERVICE_$${upper}_ELF=/workspace/target/riscv64imac-unknown-none-elf/release/$$svc"; \
		  done; \
		  env $$svc_env RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p init-lite --target riscv64imac-unknown-none-elf --release && \
		  echo "[2/2] cross build kernel (riscv) — embeds init-lite via EMBED_INIT_ELF"; \
		  rustup toolchain list | grep -q "$(NIGHTLY)" || rustup toolchain install "$(NIGHTLY)" --profile minimal; \
		  rustup component add rust-src --toolchain "$(NIGHTLY)"; \
		  rustup target add riscv64imac-unknown-none-elf --toolchain "$(NIGHTLY)"; \
		                  EMBED_INIT_ELF="/workspace/$(INIT_ELF)" RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build \
		                    --target riscv64imac-unknown-none-elf -p neuron-boot --release'
else
	@echo "==> Building workspace on host"
	@RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo build --workspace --exclude neuron --exclude neuron-boot --exclude samgrd --exclude bundlemgrd --exclude identityd --exclude dsoftbusd --exclude dist-data --exclude clipboardd --exclude notifd --exclude resmgrd --exclude searchd --exclude settingsd --exclude time-syncd --exclude netstackd
	@echo "==> Pre-compiling host test binaries (consumed by 'make test')"
	@if cargo nextest --version >/dev/null 2>&1; then \
	  RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo nextest list --workspace --exclude neuron --exclude neuron-boot >/dev/null; \
	else \
	  RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo test --workspace --exclude neuron --exclude neuron-boot --no-run; \
	fi
	@echo "==> Cross-compiling OS services (riscv64, --release) — full DEFAULT_SERVICE_LIST set so init-lite can embed all of them"
	@# Must match scripts/run-qemu-rv64.sh DEFAULT_SERVICE_LIST so `make test` /
	@# `make run` with NEXUS_SKIP_BUILD=1 finds every service ELF up-front.
	@# CRITICAL: --release is required so artifacts land under
	@#   target/$(RV_TARGET)/release/<svc>
	@# matching the INIT_LITE_SERVICE_<NAME>_ELF paths set further below
	@# and the paths qemu-test.sh + run-qemu-rv64.sh expect. Without --release
	@# init-lite's build.rs `std::fs::copy` fails on fresh checkouts (CI)
	@# with "No such file or directory" on the release path; locally a
	@# previous `just test-os` can mask the bug because it had already
	@# populated `release/`.
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build \
	  -p keystored -p rngd -p policyd -p logd -p metricsd \
	  -p samgrd -p bundlemgrd -p statefsd -p updated -p timed \
	  -p packagefsd -p vfsd -p execd -p netstackd -p dsoftbusd \
	  -p selftest-client \
	  --target $(RV_TARGET) --no-default-features --features os-lite --release
	@$(MAKE) dep-gate
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p nexus-init --lib --target $(RV_TARGET) --no-default-features --features os-lite
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p nexus-log --features sink-userspace --target $(RV_TARGET) --release
	@echo "==> Building init-lite with INIT_LITE_SERVICE_*_ELF env vars (so service ELFs are baked in)"
	@# init-lite's build.rs reads INIT_LITE_SERVICE_<NAME>_ELF for each entry in
	@# the service list and include_bytes! the ELF into the init-lite binary.
	@# Without these env vars, init-lite is built with NO services embedded and
	@# the kernel boots into a userspace that immediately page-faults.
	@svc_env=""; \
	 for svc in keystored rngd policyd logd metricsd samgrd bundlemgrd statefsd updated timed packagefsd vfsd execd netstackd dsoftbusd selftest-client; do \
	   upper=$$(echo "$$svc" | tr '[:lower:]' '[:upper:]' | tr '-' '_'); \
	   svc_env="$$svc_env INIT_LITE_SERVICE_$${upper}_ELF=$(CURDIR)/$(TARGET_DIR)/$(RV_TARGET)/release/$$svc"; \
	 done; \
	 env $$svc_env RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p init-lite --target $(RV_TARGET) --release
	@echo "==> Cross-building kernel (neuron-boot) with EMBED_INIT_ELF=$(INIT_ELF)"
	@# CRITICAL: neuron-boot's build.rs reads $$EMBED_INIT_ELF and bakes the
	@# init-lite ELF into the kernel image. Without this env var the kernel
	@# boots, finishes selftests, and then idles forever (no userspace work).
	@# Both `make build` paths (host + container) MUST set it; the script-side
	@# kernel rebuild is suppressed by NEXUS_SKIP_BUILD=1 in `make test`/`make run`.
	@rustup toolchain list | grep -q "$(NIGHTLY)" || rustup toolchain install "$(NIGHTLY)" --profile minimal
	@rustup component add rust-src --toolchain "$(NIGHTLY)" >/dev/null 2>&1 || true
	@rustup target add $(RV_TARGET) --toolchain "$(NIGHTLY)" >/dev/null 2>&1 || true
	@EMBED_INIT_ELF="$(CURDIR)/$(INIT_ELF)" RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build --target $(RV_TARGET) -p neuron-boot --release
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
		  if $(CARGO_BIN) nextest --version >/dev/null 2>&1; then \
		    RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) nextest run --workspace --exclude neuron --exclude neuron-boot; \
		  else \
		    echo "[warn] cargo-nextest not found; falling back to cargo test"; \
		    RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) test --workspace --exclude neuron --exclude neuron-boot; \
		  fi'
else
	@echo "==> Running host-first tests"
	@if cargo nextest --version >/dev/null 2>&1; then \
	  RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo nextest run --workspace --exclude neuron --exclude neuron-boot; \
	else \
	  echo "[warn] cargo-nextest not found; falling back to cargo test"; \
	  RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo test --workspace --exclude neuron --exclude neuron-boot; \
	fi
endif
	@echo "==> Running deterministic SMP ladder (smp profile, SMP=$(SMP))"
	@# Make-discipline (post TASK-0023B P5 follow-up):
	@#  - `make build` is the SOLE build step; the QEMU ladder consumes
	@#    its artifacts via NEXUS_SKIP_BUILD=1 (see scripts/run-qemu-rv64.sh).
	@#    A missing artifact fails fast with a "run make build first" hint.
	@#  - The `smp` manifest profile carries REQUIRE_SMP=1 + SMP=2 in its
	@#    env (see proof-manifest/profiles/harness.toml). `qemu-test.sh
	@#    --profile=smp` therefore enables the SMP marker subset
	@#    (`emit_when={profile="smp"}` in markers/bringup.toml); without
	@#    --profile=smp the `KSELFTEST: smp online ok` family would be
	@#    reported as "unexpected" by verify-uart. The parity ladder
	@#    runs with the `full` profile under SMP=1.
	@NEXUS_SKIP_BUILD=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh --profile=smp
	@NEXUS_SKIP_BUILD=1 SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh --profile=full

# Note: `make verify` was retired in favor of `just test-all`, which is the
# canonical aggregate gate (fmt-check + lint + deny + host tests + e2e +
# miri + arch-check + kernel build + ci-os-smp). The `make` spur stays
# self-contained (no `just` dependency) and limits itself to build/test/run.

run:
	@echo "==> Launching NEURON kernel under QEMU"
	@# Make-discipline: `make build` is the build step. `make run` only
	@# launches the QEMU smoke; NEXUS_SKIP_BUILD=1 forwards into
	@# scripts/run-qemu-rv64.sh so the kernel/init-lite/services
	@# artifacts MUST already exist (clear "run make build first" error
	@# if they don't). To pre-build inline, do `make build run`.
	@# Profile pick: SMP env wins if explicit; otherwise default to the
	@# `smp` profile when SMP>=2 (matches harness.toml SMP="2") so
	@# verify-uart accepts the SMP-only marker subset. TASK-0054 will
	@# add a `none` harness profile for fast UI-only boots; until then
	@# `smp` is the historical default.
	@profile=$${PROFILE:-}; \
	if [ -z "$$profile" ]; then \
	  smp_eff=$${SMP:-$(SMP)}; \
	  if [ "$$smp_eff" -ge 2 ] 2>/dev/null; then profile=smp; else profile=full; fi; \
	fi; \
	run_until_marker=$${RUN_UNTIL_MARKER:-1}; \
	if [ "$$run_until_marker" != "0" ]; then \
	  echo "==> RUN_UNTIL_MARKER=$$run_until_marker, --profile=$$profile (SMP from manifest if profile=smp; else SMP=$${SMP:-$(SMP)})"; \
	  NEXUS_SKIP_BUILD=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} RUN_UNTIL_MARKER=$$run_until_marker ./scripts/qemu-test.sh --profile=$$profile; \
	else \
	  UART_LOG=$${UART_LOG:-uart.log}; \
	  NEXUS_SKIP_BUILD=1 SMP=$${SMP:-$(SMP)} RUN_TIMEOUT=$${RUN_TIMEOUT:-30s} ./scripts/run-qemu-rv64.sh; \
	  status=$$?; \
	  if [ "$$status" = "124" ] && [ -f "$$UART_LOG" ] && grep -aFq "SELFTEST: end" "$$UART_LOG"; then \
	    echo "[warn] QEMU timed out, but UART log contains 'SELFTEST: end' (selftest completed)."; \
	    echo "[hint] For a truly green run, prefer: RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s make run"; \
	    exit 0; \
	  fi; \
	  exit $$status; \
	fi

dep-gate:
	@echo "==> RFC-0009 Dependency Hygiene Gate (Makefile)"
	@forbidden="parking_lot parking_lot_core getrandom"; \
	services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd timed metricsd"; \
	found=0; \
	for svc in $$services; do \
	  tree_output=$$(cargo +$(NIGHTLY) tree -p "$$svc" --target riscv64imac-unknown-none-elf --no-default-features --features os-lite 2>&1 || true); \
	  for f in $$forbidden; do \
	    echo "$$tree_output" | grep -qE "^[│├└ ]*$$f " && echo "[FAIL] $$svc pulled forbidden crate $$f" && found=1; \
	  done; \
	done; \
	test "$$found" -eq 0 && echo "[PASS] RFC-0009 dep-gate" || (echo "[FAIL] RFC-0009 dep-gate" && exit 1)

run-init-host:
	@echo "==> Running host nexus-init (will exit on init: ready)"
	@RUN_TIMEOUT=$${RUN_TIMEOUT:-30s} ./scripts/host-init-test.sh

test-init-host:
	@echo "==> Host init test"
	@./scripts/host-init-test.sh

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
