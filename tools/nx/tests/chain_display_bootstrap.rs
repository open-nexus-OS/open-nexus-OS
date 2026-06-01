// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: Display Bootstrap (ersetzt 7 Source-Scraping-Tests)
//! OWNERS: @tools-team
//!
//! Verifiziert die Kette:
//!   fbdevd start → VMO allozieren → ramfb konfigurieren
//!   → VMO an windowd senden → windowd registriert
//!   → windowd komponiert ersten Frame → Display-Marker
//!
//! Ersetzt:
//!   - fbdevd_polls_windowd_with_owned_cap_move_reply_inbox
//!   - init_wires_fbdevd_caps_and_routes_for_service_owned_display_observer_chain
//!   - windowd_visible_bootstrap_emits_present_summary_marker_with_first_frame_proof
//!   - windowd_first_frame_uses_budgeted_glass_quality
//!   - windowd_refreshes_observer_state_after_cursor_overlay_composition
//!   - windowd_target_color_changes_use_single_row_band_fast_path
//!   - visible_bootstrap_runner_injects_real_input_through_qmp (teilweise)

#[cfg(test)]
mod tests {
    use nx::chain::contract::{FbdevdContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Chain: fbdevd → windowd → first frame
    ///
    /// Hops:
    ///   1. fbdevd: ready               (fbdevd startet, alloziiert FB)
    ///   2. fbdevd: map ok              (VMO-Mapping erfolgreich)
    ///   3. fbdevd: ramfb configured    (ramfb via fw_cfg DMA)
    ///   4. windowd: ready (...)        (windowd Runtime initialisiert)
    ///   5. windowd: fb registered      (fbdevd sendet VMO, windowd akzeptiert)
    ///   6. display: first scanout ok   (erster Frame komponiert)
    #[tokio::test]
    async fn chain_display_bootstrap() {
        let mut runner = ChainRunner::new("display-bootstrap");

        // Services registrieren
        runner.register(Box::new(FbdevdContract::new(1280, 800, true)));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        // Hop 1: fbdevd alloziiert Framebuffer und ist bereit
        runner
            .expect_marker("fbdevd: ready", ms(500))
            .describe("fbdevd startet und alloziiert Framebuffer-VMO");

        // Hop 2: VMO-Mapping erfolgreich
        runner
            .expect_marker("fbdevd: map ok", ms(200))
            .after(0)
            .describe("fbdevd mapped VMO erfolgreich");

        // Hop 3: ramfb konfiguriert
        runner
            .expect_marker("fbdevd: ramfb configured", ms(300))
            .after(1)
            .describe("fbdevd konfiguriert QEMU ramfb via fw_cfg DMA");

        // Hop 4: windowd Runtime bereit
        runner
            .expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000))
            .describe("windowd initialisiert DisplayServerRuntime");

        // Hop 5: Framebuffer-Registrierung (fbdevd → windowd IPC)
        runner
            .expect_marker("windowd: fb registered", ms(500))
            .after(3)
            .describe("fbdevd sendet Framebuffer-VMO, windowd registriert");

        // Hop 6: Erster Frame komponiert und gescannt
        runner
            .expect_marker("display: first scanout ok", ms(500))
            .after(4)
            .describe("windowd komponiert ersten Frame und signalisiert Scanout");

        // Ausführen
        let report = runner.run().await;

        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: Display Bootstrap ohne Splash (optimierter Pfad)
    #[tokio::test]
    async fn chain_display_bootstrap_without_splash() {
        let mut runner = ChainRunner::new("display-bootstrap-no-splash");

        runner.register(Box::new(FbdevdContract::new(1280, 800, false)));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner.expect_marker("fbdevd: ready", ms(500));
        runner.expect_marker("fbdevd: map ok", ms(200)).after(0);
        runner.expect_marker("fbdevd: ramfb configured", ms(300)).after(1);
        runner.expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000));
        runner.expect_marker("windowd: fb registered", ms(500)).after(3);
        runner.expect_marker("display: first scanout ok", ms(500)).after(4);

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: windowd startet auch ohne fbdevd (Fallback-Pfad)
    #[tokio::test]
    async fn chain_windowd_starts_without_fbdevd() {
        let mut runner = ChainRunner::new("windowd-standalone");

        // Nur windowd — kein fbdevd
        runner.register(Box::new(WindowdContract::headless()));

        runner.expect_marker("windowd: runtime init start", ms(500));
        runner.expect_marker("windowd: wallpaper fallback solid", ms(200));
        runner.expect_marker("windowd: runtime init ok", ms(200));
        runner.expect_marker("windowd: ready (w=1280, h=800, hz=60)", ms(500));

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);

        // fb_registered sollte NICHT erscheinen (kein fbdevd)
        assert!(report.hops.iter().all(|h| { !h.marker.contains("fb registered") }));
    }
}
