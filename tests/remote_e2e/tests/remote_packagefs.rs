//! CONTEXT: Host end-to-end verification for remote packagefs read-only channel
//! INTENT: Prove STAT/OPEN/READ/CLOSE success path and reject/status mapping
//! DEPS: remote_e2e harness (FakeNet + two host nodes + in-memory packagefs backend)
//! TESTS: roundtrip success, ENOENT/BADF, path/scheme rejects, oversize read reject
#![cfg(nexus_env = "host")]

use nexus_net::fake::FakeNet;
use remote_e2e::{random_port, Node};

const PK_STATUS_OK: u8 = 0;
const PK_STATUS_PATH_TRAVERSAL: u8 = 3;
const PK_STATUS_NON_PACKAGEFS_SCHEME: u8 = 4;
const PK_STATUS_NOT_FOUND: u8 = 5;
const PK_STATUS_BADF: u8 = 6;
const PK_STATUS_OVERSIZED: u8 = 7;

fn connect_nodes() -> remote_e2e::RemoteConnection {
    let net = FakeNet::new();
    let node_a = Node::start_facade(net.clone(), random_port(), vec!["samgrd".into()]).unwrap();
    let mut announcements = node_a.watch().expect("watch registry");
    let node_b =
        Node::start_facade(net, random_port(), vec!["samgrd".into(), "packagefsd".into()]).unwrap();

    let remote =
        announcements.find(|ann| ann.device_id() == &node_b.device_id()).expect("discover node b");
    node_a.connect(&remote).expect("connect to node b")
}

#[test]
fn remote_packagefs_roundtrip_stat_open_read_close() {
    let connection = connect_nodes();
    let bytes = connection
        .remote_pkgfs_read_once("pkg:/system/build.prop", 64)
        .expect("read packagefs build.prop");
    assert_eq!(bytes, b"ro.nexus.build=dev\n");
}

#[test]
fn remote_packagefs_negative_statuses() {
    let connection = connect_nodes();

    let (stat_st, _, _) =
        connection.remote_pkgfs_stat_status("pkg:/system/missing.prop").expect("stat missing path");
    assert_eq!(stat_st, PK_STATUS_NOT_FOUND);

    let (open_st, handle) =
        connection.remote_pkgfs_open_status("pkg:/system/missing.prop").expect("open missing path");
    assert_eq!(open_st, PK_STATUS_NOT_FOUND);
    assert_eq!(handle, 0);

    let (read_st, _) =
        connection.remote_pkgfs_read_status(0xdecafbad, 0, 16).expect("read invalid handle");
    assert_eq!(read_st, PK_STATUS_BADF);

    let close_st = connection.remote_pkgfs_close_status(0xdecafbad).expect("close invalid handle");
    assert_eq!(close_st, PK_STATUS_BADF);
}

#[test]
fn remote_packagefs_rejects_invalid_paths_and_read_bounds() {
    let connection = connect_nodes();

    let (traversal_st, _, _) =
        connection.remote_pkgfs_stat_status("pkg:/../etc/passwd").expect("traversal request");
    assert_eq!(traversal_st, PK_STATUS_PATH_TRAVERSAL);

    let (scheme_st, _, _) = connection
        .remote_pkgfs_stat_status("http://example.invalid/evil")
        .expect("non pkg scheme request");
    assert_eq!(scheme_st, PK_STATUS_NON_PACKAGEFS_SCHEME);

    let (open_st, handle) =
        connection.remote_pkgfs_open_status("pkg:/system/build.prop").expect("open valid path");
    assert_eq!(open_st, PK_STATUS_OK);
    assert!(handle > 0);

    let (oversized_st, data) =
        connection.remote_pkgfs_read_status(handle, 0, 129).expect("oversized read request");
    assert_eq!(oversized_st, PK_STATUS_OVERSIZED);
    assert!(data.is_empty());
}
