fn main() {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"host\", \"os\"))");
}
