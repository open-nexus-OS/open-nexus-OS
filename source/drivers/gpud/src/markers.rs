// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Marker strings for the gpud service.
pub const GPUD_READY: &str = "gpud: ready";
pub const GPUD_VIRTIO_GPU_PROBED: &str = "gpud: virtio-gpu probed";
pub const GPUD_NO_DEVICE: &str = "gpud: no device";
pub const GPUD_CURSOR_ON: &str = "gpud: cursor on";
pub const GPUD_SCANOUT_OK: &str = "gpud: scanout ok";
pub const GPUD_SCANOUT_MODE: &str = "gpud: scanout 1280x800 bgra8888";
pub const GPUD_DISPLAY_READY: &str = "gpud: display ready (w=1280, h=800)";
pub const GPUD_MMIO_FAULT: &str = "gpud: mmio fault";
pub const GPUD_RESOURCE_VMO_CREATE_FAIL: &str = "gpud: resource vmo_create fail";
pub const GPUD_RESOURCE_VMO_MAP_FAIL: &str = "gpud: resource vmo_map_page fail";
pub const GPUD_RESOURCE_CAP_QUERY_FAIL: &str = "gpud: resource cap_query fail";
pub const GPUD_CB_RENDER_OK: &str = "gpud: cb render ok";
pub const GPUD_RESOURCE_CREATE_CMD_FAIL: &str = "gpud: resource create cmd fail";
pub const GPUD_RESOURCE_ATTACH_CMD_FAIL: &str = "gpud: resource attach cmd fail";
pub const GPUD_RESOURCE_CREATED: &str = "gpud: resource created";
pub const GPUD_IPC_READY: &str = "gpud: ipc ready";
pub const GPUD_ANIMATION_SUBMIT_OK: &str = "gpud: animation submit ok";
pub const GPUD_ANIMATION_SUBMIT_FAIL: &str = "gpud: animation submit fail";
pub const GPUD_REQUEST_MALFORMED: &str = "gpud: request malformed";
