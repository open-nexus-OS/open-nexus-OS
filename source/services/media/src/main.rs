//! CONTEXT: Media service entrypoint wiring to service logic
//! INTENT: Media playback/record pipeline, codec routing, volume
//! IDL (target): createPlayer(), setSource(url/vmo), play(), pause(), setVolume(vol)
//! DEPS: resmgrd (resources), vfsd/packagefsd (payloads)
//! READINESS: print "media: ready"; register/heartbeat with samgr
//! TESTS: createPlayer/setSource mock
fn main() {
    media::run();
}
