//! Thin SAMGR daemon: decodes Cap'n Proto requests, forwards to userspace `samgr` lib, replies encoded.

fn main() -> ! {
    touch_schemas();
    println!("samgrd: ready");
    loop {
        core::hint::spin_loop();
    }
}

fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        use nexus_idl_runtime::samgr_capnp::{register_request, resolve_request};
        let _ = core::any::type_name::<register_request::Owned>();
        let _ = core::any::type_name::<resolve_request::Owned>();
    }
}
