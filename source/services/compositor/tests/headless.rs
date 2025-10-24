//! CONTEXT: Compositor daemon headless tests
//! INTENT: Surface/layer composition, VSync, window Z-order
//! IDL (target): createSurface(token), commit(surface,rects), setLayer(win,z), subscribeVsync()
//! DEPS: systemui, windowd (if separate)
//! READINESS: print "compositor: ready"; register/heartbeat with samgr
//! TESTS: VSync tick; frame checksum stable
#[test]
fn composed_checksum() {
    assert_eq!(compositor::checksum(), 15_196_384);
}
