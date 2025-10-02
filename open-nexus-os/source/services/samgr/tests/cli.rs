#[test]
fn default_ready() {
    let result = samgr::execute(&[]);
    assert!(result.contains("ready"));
}
