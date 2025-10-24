//! CONTEXT: Nexus IDL developer tool
//! INTENT: Generate Cap'n Proto schemas and list available interfaces
//! IDL (target): gen(), list()
//! DEPS: capnpc (code generation)
//! READINESS: Command-line tool; no service dependencies
//! TESTS: Help output; future schema generation
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Nexus IDL developer tool stub.

fn main() {
    print_help();
}

fn print_help() {
    println!("nexus-idl gen   # future: invoke capnpc with project conventions");
    println!("nexus-idl list  # future: list available schemas");
}
