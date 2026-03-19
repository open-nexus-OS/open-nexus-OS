#![cfg(nexus_env = "host")]

use nexus_net::fake::FakeNet;
use remote_e2e::{random_port, Node};

#[test]
fn remote_pkgfs_roundtrip_and_negative_statuses() {
    let net = FakeNet::new();
    let port_a = random_port();
    let node_a = Node::start_facade(net.clone(), port_a, vec!["samgrd".into()]).unwrap();

    let mut announcements = node_a.watch().expect("watch registry");
    let port_b = random_port();
    let node_b = Node::start_facade(
        net,
        port_b,
        vec!["samgrd".into(), "bundlemgrd".into(), "packagefsd".into()],
    )
    .unwrap();
    node_b.register_service("bundlemgrd", 7).expect("register bundlemgrd");

    let remote =
        announcements.find(|ann| ann.device_id() == &node_b.device_id()).expect("discover node b");
    let connection = node_a.connect(&remote).expect("connect to node b");

    let bytes =
        connection.remote_pkgfs_read_once("pkg:/system/build.prop", 64).expect("pkgfs read once");
    assert!(bytes.starts_with(b"ro.nexus.build=dev"));

    let (st_missing, _, _) =
        connection.remote_pkgfs_stat_status("pkg:/system/does-not-exist").expect("stat missing");
    assert_eq!(st_missing, 5, "expected PK_STATUS_NOT_FOUND");

    let (st_traversal, _, _) =
        connection.remote_pkgfs_stat_status("pkg:/system/../secret").expect("stat traversal");
    assert_eq!(st_traversal, 3, "expected PK_STATUS_PATH_TRAVERSAL");

    let (st_scheme, _, _) =
        connection.remote_pkgfs_stat_status("file:/etc/passwd").expect("stat non-packagefs");
    assert_eq!(st_scheme, 4, "expected PK_STATUS_NON_PACKAGEFS_SCHEME");

    let long_rel = "a".repeat(193);
    let long_path = format!("pkg:/{}", long_rel);
    let (st_oversized, _, _) = connection.remote_pkgfs_stat_status(&long_path).expect("stat oversized");
    assert_eq!(st_oversized, 7, "expected PK_STATUS_OVERSIZED");

    let (st_open, handle) =
        connection.remote_pkgfs_open_status("pkg:/system/build.prop").expect("open build.prop");
    assert_eq!(st_open, 0, "expected PK_STATUS_OK");
    let st_close = connection.remote_pkgfs_close_status(handle).expect("close");
    assert_eq!(st_close, 0, "expected PK_STATUS_OK close");
    let st_badf = connection.remote_pkgfs_close_status(handle).expect("close badf");
    assert_eq!(st_badf, 6, "expected PK_STATUS_BADF");

    let mut open_handles = Vec::new();
    for _ in 0..8 {
        let (st, h) = connection
            .remote_pkgfs_open_status("pkg:/system/build.prop")
            .expect("open for limit");
        assert_eq!(st, 0, "expected PK_STATUS_OK while below limit");
        open_handles.push(h);
    }
    let (st_limit, _) = connection
        .remote_pkgfs_open_status("pkg:/system/build.prop")
        .expect("open at limit");
    assert_eq!(st_limit, 8, "expected PK_STATUS_LIMIT");
    for h in open_handles {
        let st = connection.remote_pkgfs_close_status(h).expect("close opened handle");
        assert_eq!(st, 0, "expected PK_STATUS_OK close from handle set");
    }
}
