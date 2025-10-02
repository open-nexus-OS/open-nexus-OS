#[test]
fn default_execution() {
    let result = abilitymgr::execute(&[]);
    assert!(result.contains("ready"));
}
