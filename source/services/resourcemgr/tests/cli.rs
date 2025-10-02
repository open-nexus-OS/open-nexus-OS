#[test]
fn host_path() {
    let result = resourcemgr::execute(&[]);
    assert!(result.contains("initialized"));
}
