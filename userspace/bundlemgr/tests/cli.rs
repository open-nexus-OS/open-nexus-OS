struct StubRegistrar;

impl bundlemgr::AbilityRegistrar for StubRegistrar {
    fn register(&self, ability: &str) -> Result<Vec<u8>, String> {
        Ok(vec![ability.len() as u8])
    }
}

#[test]
fn install_flow() {
    let output = bundlemgr::execute(&["install", "apps/test-signed.nxb"], &StubRegistrar);
    assert!(output.contains("bundle installed"));
}

#[test]
fn remove_flow() {
    let output = bundlemgr::execute(&["remove", "launcher"], &StubRegistrar);
    assert!(output.contains("removed"));
}
