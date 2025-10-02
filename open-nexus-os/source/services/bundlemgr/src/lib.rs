use nexus_abi::MsgHeader;
use nexus_idl::nexus_interface;

nexus_interface!(interface ability_ipc {
    fn register(&self, ability: &str) -> bool;
});

struct AbilityProxy;

impl ability_ipc::Service for AbilityProxy {
    fn register(&self, ability: &str) -> bool {
        !ability.is_empty()
    }
}

pub fn help() -> &'static str {
    "bundlemgr installs Nexus bundles. Usage: bundlectl <install|remove|query> [args]"
}

pub fn execute(args: &[&str]) -> String {
    if args.is_empty() || args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    match args[0] {
        "install" => {
            if let Some(path) = args.get(1) {
                install(path)
            } else {
                "missing bundle path".to_string()
            }
        }
        "remove" => {
            if let Some(name) = args.get(1) {
                format!("bundle {name} removed")
            } else {
                "missing bundle name".to_string()
            }
        }
        "query" => "bundles: launcher".to_string(),
        other => format!("unknown command {other}"),
    }
}

fn install(path: &str) -> String {
    if !path.ends_with(".nxb") {
        return "invalid bundle format".to_string();
    }
    if !verify_signature(path) {
        return "signature check failed".to_string();
    }
    let header = MsgHeader::new(1, 0, 0);
    let ability = infer_ability(path);
    if ability_ipc::Service::register(&AbilityProxy, &ability) {
        format!("bundle installed: {path} with header {:?}", header.serialize())
    } else {
        "ability registration failed".to_string()
    }
}

fn verify_signature(path: &str) -> bool {
    path.contains("signed")
}

fn infer_ability(path: &str) -> String {
    path.split('/').last().unwrap_or("bundle").replace(".nxb", "")
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, infer_ability, install, verify_signature};

    #[test]
    fn signature_requires_marker() {
        assert!(verify_signature("app-signed.nxb"));
        assert!(!verify_signature("app.nxb"));
    }

    #[test]
    fn install_checks_extension() {
        assert_eq!(install("bad"), "invalid bundle format");
    }

    #[test]
    fn install_success_path() {
        let output = execute(&["install", "apps/launcher-signed.nxb"]);
        assert!(output.contains("bundle installed"));
        assert_eq!(infer_ability("apps/launcher-signed.nxb"), "launcher-signed");
    }
}
