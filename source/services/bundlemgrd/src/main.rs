//! Bundle manager daemon wiring: forwards CLI requests to the userspace library.

use bundlemgr::{run_with, AbilityRegistrar};
use nexus_abi::MsgHeader;
use nexus_idl::nexus_interface;

nexus_interface!(interface ability_ipc {
    fn register(&self, ability: &str) -> bool;
});

struct ServiceRegistrar;

impl ability_ipc::Service for ServiceRegistrar {
    fn register(&self, ability: &str) -> bool {
        !ability.is_empty()
    }
}

impl AbilityRegistrar for ServiceRegistrar {
    fn register(&self, ability: &str) -> Result<Vec<u8>, String> {
        if ability_ipc::Service::register(self, ability) {
            let header = MsgHeader::new(1, 0, 0);
            Ok(header.serialize().to_vec())
        } else {
            Err("samgr rejected ability".into())
        }
    }
}

fn main() {
    let registrar = ServiceRegistrar;
    run_with(&registrar);
    println!("bundlemgrd: ready");
}
