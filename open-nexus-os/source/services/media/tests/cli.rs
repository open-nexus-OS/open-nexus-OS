#[test]
fn probe_ready() {
    assert!(media::execute(&["--probe", "clip.mp4"]).contains("clip.mp4"));
}
