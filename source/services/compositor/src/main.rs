//! CONTEXT: Compositor daemon entrypoint wiring to service logic
//! INTENT: Surface/layer composition, vsync, window z-order
//! IDL (target): createSurface(token), commit(surface, rects), setLayer(win,z), subscribeVsync()
//! DEPS: systemui/windowd; vfsd (assets)
//! READINESS: print "compositor: ready"; register/heartbeat with samgr
//! TESTS: vsync tick; surface commit mock
fn main() {
    compositor::run();
}
