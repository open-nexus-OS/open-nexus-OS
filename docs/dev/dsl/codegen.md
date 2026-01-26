<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# AOT Codegen

Interpreter mode is for iteration. AOT mode exists to improve startup and reduce runtime overhead.

## Contract

- input: canonical `.nxir` (Cap'n Proto)
- output: generated Rust crate(s)
- parity: interpreter and AOT outputs must match for the same `{profile, locale}` inputs (goldens prove it)
