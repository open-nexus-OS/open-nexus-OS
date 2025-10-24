//! CONTEXT: Window manager daemon headless tests
//! INTENT: Window/surface management, compositor bridge, Z-order
//! IDL (target): createWindow(spec), setZOrder(id,z), resize(id,w,h), close(id)
//! DEPS: compositor (rendering), systemui (chrome)
//! READINESS: print "windowd: ready"; register/heartbeat with samgr
//! TESTS: createWindow mock; frame checksum stable
#[test]
fn headless_checksum() {
    assert_eq!(windowd::frame_checksum(), 14_680_288);
}
