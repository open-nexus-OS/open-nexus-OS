// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the policy the system boots with (`policies/`), checked as committed: its manifest
//! names the tree it was written from (an edit without the authoring step
//! `nx policy validate --write-manifest` fails here, not months later — the manifest had gone
//! stale across three edits before this test existed), and the disk has ONE holder (ADR-0044,
//! ADR-0067, TASK-0246 P4b): only the block owner may be granted a virtio disk or an SD/MMC
//! host, so a second service that asked would be denied by policyd — likewise the display plane
//! (gpud), the USB host controller (xhcid) and the USB HID class it serves (hidrawd).
//! OWNERS: @runtime @security

use std::path::Path;

use nexus_policy::PolicyTree;

fn committed() -> PolicyTree {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    PolicyTree::load_single_authority(&repo).expect("the committed policy loads")
}

#[test]
fn the_committed_manifest_names_the_committed_tree() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    committed()
        .validate_manifest(&repo.join("policies"))
        .expect("policies/manifest.json is stale: run `nx policy validate --root policies --write-manifest`");
}

/// TASK-0251 P2: the board's display plane (controller + encoder windows) has one holder — the
/// display owner; its client (windowd), the harness and the glue owner are denied.
#[test]
fn test_reject_a_second_holder_of_the_display() {
    let tree = committed();
    let holders: Vec<&str> = tree.policy().holders("device.mmio.display").collect();
    assert_eq!(holders, ["gpud"]);
    for subject in ["windowd", "selftest-client", "socd", "inputd", "hidrawd"] {
        assert!(tree.policy().check(&["device.mmio.display"], subject).is_err(), "{subject}");
    }
}

/// TASK-0328 U1 (RFC-0099): the USB host controller has one holder — its owner; the HID
/// client (hidrawd), the input router, the harness and the glue owner are denied.
#[test]
fn test_reject_a_second_holder_of_the_usb_host() {
    let tree = committed();
    let holders: Vec<&str> = tree.policy().holders("device.mmio.usb").collect();
    assert_eq!(holders, ["xhcid"]);
    for subject in ["hidrawd", "inputd", "selftest-client", "socd", "gpud"] {
        assert!(tree.policy().check(&["device.mmio.usb"], subject).is_err(), "{subject}");
    }
}

/// TASK-0253B (RFC-0099 §5): xhcid serves the HID boot class to ONE subscriber, admitted by
/// `usb.hid` — keystrokes go to the HID client and nowhere else; the input router, the harness,
/// the controller's owner itself and the glue owner are denied.
#[test]
fn test_reject_a_second_subscriber_of_usb_hid() {
    let tree = committed();
    let holders: Vec<&str> = tree.policy().holders("usb.hid").collect();
    assert_eq!(holders, ["hidrawd"]);
    for subject in ["inputd", "selftest-client", "xhcid", "socd", "windowd"] {
        assert!(tree.policy().check(&["usb.hid"], subject).is_err(), "{subject}");
    }
}

#[test]
fn test_reject_a_second_holder_of_the_disk() {
    let tree = committed();
    for class in ["device.mmio.blk", "device.mmio.mmc"] {
        let holders: Vec<&str> = tree.policy().holders(class).collect();
        assert_eq!(holders, ["blkd"], "{class}");
    }
    // The clients of the block plane and a service that asks for everything are denied.
    for subject in
        ["statefsd", "vfsd", "bundlemgrd", "updated", "bootctld", "netstackd", "selftest-client"]
    {
        assert!(tree.policy().check(&["device.mmio.blk"], subject).is_err(), "{subject}");
        assert!(tree.policy().check(&["device.mmio.mmc"], subject).is_err(), "{subject}");
    }
}
