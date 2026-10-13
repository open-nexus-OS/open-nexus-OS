// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The node classes of RFC-0106's per-class floor (TASK-0329 G1, "Input from Block 1"):
//! a consumer may bring up only the node classes policyd allows it — `soc.glue.<class>`,
//! deny-by-default — so blkd brings up storage nodes alone, gpud display and GPU nodes, xhcid
//! USB nodes. The class is the node's, read from its compatible, as data here: one table, the
//! board's compatibles, no consumer carries its own copy. A node whose compatible the table does
//! not place has no class; `socd` refuses its bring-up when its plan would touch a register
//! (a node that needs nothing — QEMU's virtio transports — is `NotNeeded` without a class).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; `socd`'s contract tests (`test_reject_soc_glue_class_*`)
//! INVARIANTS: every class name is a capability suffix (`soc.glue.<class>`) policyd's grammar
//!   accepts; a compatible maps to at most one class.

use nexus_fdt::Node;

/// A node class — the suffix of its `soc.glue.<class>` capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Storage,
    Display,
    Gpu,
    Usb,
}

impl Class {
    /// The capability suffix.
    pub const fn name(self) -> &'static str {
        match self {
            Class::Storage => "storage",
            Class::Display => "display",
            Class::Gpu => "gpu",
            Class::Usb => "usb",
        }
    }

    /// The capability a requester must hold to bring a node of this class up, as bytes
    /// (`b"soc.glue.storage"`).
    pub const fn capability(self) -> &'static [u8] {
        match self {
            Class::Storage => b"soc.glue.storage",
            Class::Display => b"soc.glue.display",
            Class::Gpu => b"soc.glue.gpu",
            Class::Usb => b"soc.glue.usb",
        }
    }

    /// Every class.
    pub const ALL: [Class; 4] = [Class::Storage, Class::Display, Class::Gpu, Class::Usb];
}

/// The board compatibles and their classes (the tree's data; a vendor string is a compatible).
const CLASSES: &[(&str, Class)] = &[
    ("spacemit,k1-sdhci", Class::Storage),
    ("spacemit,dpu-online2", Class::Display),
    ("spacemit,hdmi", Class::Display),
    ("img,rgx", Class::Gpu),
    ("img,img-rogue", Class::Gpu),
    ("snps,dwc3", Class::Usb),
    ("spacemit,usb2-phy", Class::Usb),
    ("spacemit,k1x-combphy", Class::Usb),
    ("spacemit,usb3-hub", Class::Usb),
];

/// The class of a compatible string, if the table places it.
pub fn of_compatible(compatible: &str) -> Option<Class> {
    CLASSES.iter().find(|(c, _)| *c == compatible).map(|(_, class)| *class)
}

/// The class of `node`: the first of its compatibles the table places.
pub fn of(node: Node<'_>) -> Option<Class> {
    node.compatible().find_map(of_compatible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_compatibles_have_their_classes() {
        assert_eq!(of_compatible("spacemit,k1-sdhci"), Some(Class::Storage));
        assert_eq!(of_compatible("spacemit,dpu-online2"), Some(Class::Display));
        assert_eq!(of_compatible("spacemit,hdmi"), Some(Class::Display));
        assert_eq!(of_compatible("img,rgx"), Some(Class::Gpu));
        assert_eq!(of_compatible("snps,dwc3"), Some(Class::Usb));
        assert_eq!(of_compatible("spacemit,usb3-hub"), Some(Class::Usb));
    }

    #[test]
    fn test_reject_a_compatible_the_table_does_not_place() {
        assert_eq!(of_compatible("virtio,mmio"), None);
        assert_eq!(of_compatible("spacemit,k1-rtc"), None);
        assert_eq!(of_compatible(""), None);
    }

    #[test]
    fn every_class_is_a_capability_suffix() {
        for class in Class::ALL {
            let cap = core::str::from_utf8(class.capability()).unwrap();
            assert_eq!(cap.strip_prefix("soc.glue."), Some(class.name()));
        }
    }
}
