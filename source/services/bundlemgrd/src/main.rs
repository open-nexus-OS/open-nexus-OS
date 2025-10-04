//! Bundle manager daemon stub that prepares Cap'n Proto plumbing and forwards requests to the userspace library.

use bundlemgr::{run_with, AbilityRegistrar};

struct StubRegistrar;

impl AbilityRegistrar for StubRegistrar {
    fn register(&self, ability: &str) -> Result<Vec<u8>, String> {
        if ability.is_empty() {
            Err("missing ability".into())
        } else {
            Ok(vec![ability.len() as u8])
        }
    }
}

fn main() -> ! {
    touch_schemas();
    run_with(&StubRegistrar);
    println!("bundlemgrd: ready");
    loop {
        core::hint::spin_loop();
    }
}

fn touch_schemas() {
    #[cfg(feature = "nexus-idl-runtime/capnp")]
    {
        use nexus_idl_runtime::bundlemgr_capnp::{install_request, install_response};
        let _ = core::any::type_name::<install_request::Owned>();
        let _ = core::any::type_name::<install_response::Owned>();
    }
}
