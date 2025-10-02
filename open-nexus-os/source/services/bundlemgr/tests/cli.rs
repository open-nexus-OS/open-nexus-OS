#[test]
fn install_flow() {
    let output = bundlemgr::execute(&["install", "apps/test-signed.nxb"]);
    assert!(output.contains("bundle installed"));
}

#[test]
fn remove_flow() {
    let output = bundlemgr::execute(&["remove", "launcher"]);
    assert!(output.contains("removed"));
}
