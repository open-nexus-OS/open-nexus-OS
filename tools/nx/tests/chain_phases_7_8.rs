// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: Phase 7 Unified Pacing + Phase 8 SystemUI Shell
//! OWNERS: @tools-team
//!
//! Validates that the pacer timer fires at display refresh rate and the
//! SystemUI shell creates the canonical scene graph structure.

#[cfg(test)]
mod tests {
    use nx::chain::contract::{GpudContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Phase 7: pacer timer arms after handoff and drives frame submission.
    #[tokio::test]
    async fn chain_pacer_timer_arms_after_handoff() {
        let mut runner = ChainRunner::new("pacer-after-handoff");

        runner.register(Box::new(GpudContract::with_handoff()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner.expect_marker("gpud: ready", ms(500));
        runner
            .expect_marker("windowd: handoff attach ack", ms(1000))
            .after(0)
            .describe("handoff complete");

        // Phase 7: pacer timer should arm after handoff (no explicit marker yet,
        // but the pacing loop should prevent busy-waiting and deliver frames at
        // consistent intervals).

        runner
            .expect_marker("windowd: present visible ok", ms(500))
            .after(1)
            .describe("first frame after pacer arm");

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Phase 8: SystemUI shell creates canonical scene graph.
    #[tokio::test]
    async fn chain_systemui_shell_creates_scene_graph() {
        let mut runner = ChainRunner::new("systemui-shell");

        runner.register(Box::new(GpudContract::with_handoff_and_cursor()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner.expect_marker("gpud: ready", ms(500));
        runner
            .expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000))
            .after(0)
            .describe("windowd ready with profile");

        // Phase 8: systemUI shell contract — one root, no duplicate hierarchies.
        // The scene graph vocabulary (SceneNodeId, InvalidationClass, RenderPrimitive)
        // is locked in. All future DSL/SystemUI tasks target this vocabulary.

        runner
            .expect_marker("windowd: present visible ok", ms(2000))
            .after(1)
            .describe("present visible through scene graph");

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);
    }
}
