//! CONTEXT: Identity daemon entrypoint wiring to service logic
//! INTENT: Device ID, sign/verify API for userland
//! IDL (target): getDeviceId(), sign(payload), verify(payload, signature, key)
//! DEPS: keystored (anchors), nexus-idl-runtime (capnp)
//! READINESS: print "identityd: ready"; register/heartbeat with samgr
//! TESTS: sign/verify loopback

fn main() -> ! {
    identityd::touch_schemas();
    if let Err(err) = identityd::service_main_loop(identityd::ReadyNotifier::new(|| ())) {
        eprintln!("identityd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
