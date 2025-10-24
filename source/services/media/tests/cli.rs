//! CONTEXT: Media service CLI tests
//! INTENT: Media playback/record pipeline, codec routing, volume
//! IDL (target): createPlayer(), setSource(url/vmo), play(), pause(), setVolume(vol)
//! DEPS: resmgrd (resources), vfsd/packagefsd (payloads)
//! READINESS: print "media: ready"; register/heartbeat with samgr
//! TESTS: createPlayer/setSource mock
#[test]
fn probe_ready() {
    assert!(media::execute(&["--probe", "clip.mp4"]).contains("clip.mp4"));
}
