//! CONTEXT: System UI service headless tests
//! INTENT: Statusbar, launcher, shell UI
//! IDL (target): showLauncher(), setStatus(icon,state), setTheme(theme)
//! DEPS: compositor/ime/notifd
//! READINESS: print "systemui: ready"; register/heartbeat with samgr
//! TESTS: show launcher mock; frame checksum stable
#[test]
fn systemui_checksum() {
    assert_eq!(systemui::checksum(), 182_315_734);
}
