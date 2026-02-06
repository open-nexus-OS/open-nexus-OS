{
  description = "NEURON kernel development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.11";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            toolchain
            pkgs.just
            pkgs.qemu
            pkgs.ninja
            pkgs.meson
            pkgs.python3
            pkgs.git
            pkgs.pkg-config
            pkgs.gnumake
            pkgs.capnproto           # provides `capnp` for Cap’n Proto codegen
            pkgs.flatbuffers         # provides `flatc` (for dein Hybrid-Setup)
            pkgs.mold                # (optional) faster linker; matches Dockerfile
            pkgs.lld                 # (optional) lld on host
          ];
          RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
        };
      });
}
