//! CONTEXT: DSoftBus daemon entrypoint wiring to service logic
//! INTENT: Discovery, session mgmt, byte/stream transport between devices
//! IDL (target): publishService(name), startDiscovery(domain), openSession(svc), sendBytes(h,buf)
//! DEPS: policyd (net), identityd (sign), samgrd
//! READINESS: print "dsoftbusd: ready"; register/heartbeat with samgr
//! TESTS: open/send/recv loopback
//! Distributed softbus daemon entry point.

fn main() {
    dsoftbus::run();
}
