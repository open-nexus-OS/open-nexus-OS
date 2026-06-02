// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: Display Bootstrap (GPU-only architecture, RFC-0059 Phase 6)
//! OWNERS: @tools-team
//!
//! Verifiziert die GPU-only Kette:
//!   gpud start → virtio-gpu probe → ready
//!   windowd start → vmo_create → compose → present → gpud scanout handoff
//!
//! Ersetzt fbdevd/ramfb-basierte Chain-Tests (TASK-0062 Phase 6 GPU-only migration).

#[cfg(test)]
mod tests {
    use nx::chain::contract::{GpudContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Chain: gpud → windowd → first frame (GPU-only self-bootstrap)
    ///
    /// Hops:
    ///   1. gpud: virtio-gpu probed       (gpud mapped MMIO, verified device id)
    ///   2. gpud: ready                   (gpud probe complete, IPC-ready)
    ///   3. windowd: ready (…)            (windowd Runtime initialisiert, wallpaper geladen)
    ///   4. windowd: backend=gpu          (windowd creates own framebuffer VMO, no handoff)
    ///   5. windowd: compose ready        (erster Frame in eigenes VMO komponiert)
    ///   6. windowd: present visible ok   (present in GPU scanout VMO abgeschlossen)
    ///   7. gpud: scanout ok              (gpud attach_backing + set_scanout)
    ///   8. SELFTEST: ui visible present ok (observer bestätigt)
    #[tokio::test]
    async fn chain_gpu_display_bootstrap() {
        let mut runner = ChainRunner::new("gpu-display-bootstrap");

        // Services registrieren (GPU-only: gpud + windowd)
        runner.register(Box::new(GpudContract::probe_only()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        // Hop 1: gpud probed virtio-gpu device
        runner
            .expect_marker("gpud: virtio-gpu probed", ms(500))
            .describe("gpud mapped virtio-gpu MMIO and verified device id");

        // Hop 2: gpud ready
        runner
            .expect_marker("gpud: ready", ms(300))
            .after(0)
            .describe("gpud probe complete, IPC-ready");

        // Hop 3: windowd Runtime bereit
        runner
            .expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000))
            .describe("windowd initialisiert DisplayServerRuntime mit Wallpaper");

        // Hop 4: windowd self-bootstrap — backend=gpu (vmo_create)
        runner
            .expect_marker("windowd: backend=gpu", ms(200))
            .after(1)
            .describe("windowd creates own framebuffer VMO, GPU-only self-bootstrap");

        // Hop 5: windowd compose ready
        runner
            .expect_marker("windowd: compose ready", ms(500))
            .after(2)
            .describe("windowd komponiert ersten Frame in eigenes VMO");

        // Hop 6: windowd present visible ok
        runner
            .expect_marker("windowd: present visible ok", ms(300))
            .after(3)
            .describe("windowd present in GPU scanout VMO abgeschlossen");

        // Hop 7: gpud scanout ok (attach_backing + set_scanout via virtio-gpu)
        runner
            .expect_marker("gpud: scanout ok", ms(500))
            .after(4)
            .describe("gpud completed ATTACH_BACKING + SET_SCANOUT");

        // Hop 8: SELFTEST observer summary
        runner
            .expect_marker("SELFTEST: ui visible present ok", ms(300))
            .after(5)
            .describe("selftest observer confirmed visible present in GPU path");

        let report = runner.run().await;

        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: windowd startet auch ohne gpud (headless, GPU-unavailable fallback)
    #[tokio::test]
    async fn chain_windowd_starts_without_gpud() {
        let mut runner = ChainRunner::new("windowd-headless");

        // Nur windowd — kein gpud
        runner.register(Box::new(WindowdContract::headless()));

        runner.expect_marker("windowd: runtime init start", ms(500));
        runner.expect_marker("windowd: wallpaper fallback solid", ms(200));
        runner.expect_marker("windowd: runtime init ok", ms(200));
        runner.expect_marker("windowd: ready (w=1280, h=800, hz=60)", ms(500));

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);

        // Keine GPU-spezifischen Marker im headless-Modus
        assert!(report.hops.iter().all(|h| {
            !h.marker.contains("backend=gpu")
                && !h.marker.contains("gpud: ready")
                && !h.marker.contains("gpud: scanout ok")
        }));
    }
}
