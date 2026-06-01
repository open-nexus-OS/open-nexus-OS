// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: GPU-Scanout-Pfad
//! OWNERS: @tools-team
//!
//! Verifiziert: gpud probe → resource create → scanout → windowd FB handoff

#[cfg(test)]
mod tests {
    use nx::chain::contract::{FbdevdContract, GpudContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Chain: gpud startet und setzt Scanout (Erfolgspfad)
    #[tokio::test]
    async fn chain_gpu_scanout_success() {
        let mut runner = ChainRunner::new("gpu-scanout");

        runner.register(Box::new(GpudContract::probe_only()));
        runner.register(Box::new(FbdevdContract::new(1280, 800, true)));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner
            .expect_marker("gpud: virtio-gpu probed", ms(500))
            .describe("gpud probed virtio-gpu MMIO device");
        runner
            .expect_marker("gpud: scanout 1280x800 bgra8888", ms(500))
            .after(0)
            .describe("gpud setzt 1280x800 Scanout");
        runner.expect_marker("gpud: display ready (w=1280, h=800)", ms(500)).after(1);
        runner.expect_marker("gpud: ready", ms(200)).after(2);

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: gpud Resource-Creation schlägt fehl → Fallback-Pfad
    #[tokio::test]
    async fn chain_gpu_resource_fails_graceful_degradation() {
        let mut runner = ChainRunner::new("gpu-fallback");

        runner.register(Box::new(GpudContract::failing_resource()));
        runner.register(Box::new(FbdevdContract::new(1280, 800, true)));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner.expect_marker("gpud: virtio-gpu probed", ms(500));
        runner.expect_marker("gpud: resource create cmd fail", ms(500)).after(0);
        runner.expect_marker("gpud: mmio fault", ms(200)).after(1);
        // Nach Fallback: gpud bleibt alive (gpud: ready)
        runner.expect_marker("gpud: ready", ms(200)).after(2);

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
        // scanout-Marker sollten NICHT erscheinen (Resource-Creation failed)
        assert!(report.hops.iter().all(|h| !h.marker.contains("scanout")));
    }
}
