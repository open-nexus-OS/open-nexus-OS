<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Incremental Builds

Incremental builds are based on stable content hashes and deterministic file/module naming.

Goals:

- only changed inputs rebuild
- stable outputs for stable inputs
- tree-shaking from reachable routes
