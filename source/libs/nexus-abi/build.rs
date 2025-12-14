fn main() {
    // Allow custom cfg used across os/host builds.
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
