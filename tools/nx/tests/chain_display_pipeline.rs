// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: Full Display Pipeline (Phases 1-6)
//! OWNERS: @tools-team
//!
//! Validates the complete GPU-first display chain with per-hop markers:
//!   gpud probe → windowd VMO create → handoff → GPU blur → present →
//!   input routing → cursor move (BlendCursor software path)
//!
//! Cursor architecture: hardware cursor upload (OP_UPLOAD_CURSOR) is disabled
//! due to QEMU virtio-gpu quirk (UPDATE_CURSOR corrupts scanout resource).
//! Cursor renders via BlendCursor embedded in every frame CommandBuffer.
//!
//! Each hop has a short deterministic timeout. A failed hop pinpoints
//! exactly where the chain breaks — no guessing.

#[cfg(test)]
mod tests {
    use nx::chain::contract::{GpudContract, InputdContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Full pipeline: gpud probe → handoff → GPU blur → BlendCursor → input
    ///
    /// Hop markers (in order):
    ///   H0: gpud: virtio-gpu probed
    ///   H1: gpud: ready
    ///   H2: windowd: runtime init ok
    ///   H3: windowd: ready (w=1280, h=800, hz=120)
    ///   H4: windowd: fb vmo create ok
    ///   H5: windowd: handoff attach sent
    ///   H6: gpud: recv OP_SET_FRAMEBUFFER_VMO
    ///   H7: windowd: handoff attach ack
    ///   H8: windowd: handoff present sent
    ///   H9: gpud: cursor on          ← BlendCursor path active (no cursor upload)
    ///  H10: windowd: present visible ok
    ///  H11: SELFTEST: ui v2 present ok
    ///  H12: windowd: cursor move visible
    ///  H13: windowd: hover visible
    ///  H14: SELFTEST: ui visible input ok
    #[tokio::test]
    async fn chain_full_display_pipeline() {
        let mut runner = ChainRunner::new("full-display-pipeline");

        runner.register(Box::new(GpudContract::with_handoff_and_cursor()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        // --- gpud probe ---
        runner
            .expect_marker("gpud: virtio-gpu probed", ms(500))
            .describe("H0: gpud probed virtio-gpu MMIO device");
        runner.expect_marker("gpud: ready", ms(300)).after(0).describe("H1: gpud IPC-ready");

        // --- windowd init ---
        runner
            .expect_marker("windowd: runtime init ok", ms(500))
            .describe("H2: windowd runtime initialized");
        runner
            .expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000))
            .after(2)
            .describe("H3: windowd ready with wallpaper");

        // --- VMO create ---
        runner
            .expect_marker("windowd: fb vmo create ok", ms(200))
            .after(3)
            .describe("H4: 16MB framebuffer VMO created");

        // --- handoff (reactive, no polling) ---
        runner
            .expect_marker("windowd: handoff attach sent", ms(300))
            .after(4)
            .describe("H5: VMO cap-move sent to gpud (blocking)");
        runner
            .expect_marker("gpud: recv OP_SET_FRAMEBUFFER_VMO", ms(500))
            .after(5)
            .describe("H6: gpud received VMO");
        runner
            .expect_marker("windowd: handoff attach ack", ms(500))
            .after(6)
            .describe("H7: gpud acknowledged VMO");
        runner
            .expect_marker("windowd: handoff present sent", ms(300))
            .after(7)
            .describe("H8: first frame present sent");

        // --- BlendCursor active (from OP_SET_FRAMEBUFFER_VMO ack path) ---
        runner
            .expect_marker("gpud: cursor on", ms(300))
            .after(8)
            .describe("H9: BlendCursor path active; no cursor resource upload");

        // --- present visible ---
        runner
            .expect_marker("windowd: present visible ok", ms(300))
            .after(9)
            .describe("H10: frame visible on scanout");
        runner
            .expect_marker("SELFTEST: ui v2 present ok", ms(300))
            .after(10)
            .describe("H11: observer confirms visible present");

        // --- input pipeline ---
        runner
            .expect_marker("windowd: cursor move visible", ms(2000))
            .after(11)
            .describe("H12: cursor move rendered via BlendCursor in CB");
        runner
            .expect_marker("windowd: hover visible", ms(1000))
            .after(12)
            .describe("H13: hover target highlighted");
        runner
            .expect_marker("SELFTEST: ui visible input ok", ms(500))
            .after(13)
            .describe("H14: observer confirms visible input");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Smoke: just the handoff chain (gpud → windowd → VMO → present)
    #[tokio::test]
    async fn chain_handoff_smoke() {
        let mut runner = ChainRunner::new("handoff-smoke");

        runner.register(Box::new(GpudContract::with_handoff()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner.expect_marker("gpud: ready", ms(500)).describe("gpud ready");
        runner.expect_marker("windowd: fb vmo create ok", ms(500)).after(0).describe("VMO created");
        runner
            .expect_marker("windowd: handoff attach sent", ms(300))
            .after(1)
            .describe("handoff sent");
        runner.expect_marker("gpud: handoff attach ack", ms(500)).after(2).describe("gpud ack");
        runner
            .expect_marker("windowd: handoff attach ack", ms(300))
            .after(3)
            .describe("windowd got ack");
        runner
            .expect_marker("windowd: present visible ok", ms(500))
            .after(4)
            .describe("present visible");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Software cursor BlendCursor path — validates cursor renders without a hardware
    /// cursor resource. The cursor is embedded as BlendCursor in every frame CB;
    /// gpud's blend_cursor_vmo draws it procedurally on the VMO each frame.
    ///
    /// Hardware cursor upload (OP_UPLOAD_CURSOR) is intentionally absent here:
    /// disabled due to QEMU virtio-gpu UPDATE_CURSOR→RESOURCE_FLUSH corruption.
    #[tokio::test]
    async fn chain_software_cursor_blend_path() {
        let mut runner = ChainRunner::new("sw-cursor-blend-path");

        runner.register(Box::new(GpudContract::with_handoff_and_cursor()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner.expect_marker("gpud: ready", ms(500));
        runner
            .expect_marker("windowd: handoff attach ack", ms(1000))
            .after(0)
            .describe("handoff done");
        // gpud: cursor on fires from OP_SET_FRAMEBUFFER_VMO ack — proves BlendCursor
        // path is the active path (no separate cursor resource uploaded).
        runner
            .expect_marker("gpud: cursor on", ms(300))
            .after(1)
            .describe("BlendCursor active; cursor embedded in frame CB");
        runner
            .expect_marker("windowd: present visible ok", ms(500))
            .after(2)
            .describe("frame rendered with BlendCursor composite");
        runner
            .expect_marker("windowd: cursor move visible", ms(2000))
            .after(3)
            .describe("cursor move triggers CB with updated BlendCursor position");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// inputd → windowd cursor pipeline: verifies that inputd's priority-wired
    /// IPC slots push visible-state to windowd and cursor move is rendered.
    #[tokio::test]
    async fn chain_inputd_cursor_pipeline() {
        let mut runner = ChainRunner::new("inputd-cursor-pipeline");

        runner.register(Box::new(GpudContract::with_handoff_and_cursor()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));
        runner.register(Box::new(InputdContract::with_cursor_moves()));

        runner.expect_marker("gpud: ready", ms(500));
        runner
            .expect_marker("windowd: present visible ok", ms(1500))
            .after(0)
            .describe("first frame visible before input");
        runner
            .expect_marker("inputd: priority-wired slots 5/6 ok", ms(300))
            .after(1)
            .describe("inputd uses init-assigned IPC slots");
        runner
            .expect_marker("inputd: cursor move computed", ms(200))
            .after(2)
            .describe("pointer-accel computed");
        runner
            .expect_marker("inputd: windowd visible-state pushed", ms(200))
            .after(3)
            .describe("visible-state pushed to windowd");
        runner
            .expect_marker("windowd: cursor move visible", ms(1000))
            .after(4)
            .describe("cursor move renders via BlendCursor in next frame CB");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }
}
