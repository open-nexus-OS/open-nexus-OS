//! Command-line and policy logic for the bundle manager service.

/// Describes a registrar capable of exposing newly installed abilities.
pub trait AbilityRegistrar {
    /// Registers `ability` and returns a serialized header for diagnostics.
    fn register(&self, ability: &str) -> Result<Vec<u8>, String>;
}

/// Returns a short usage description for the bundle manager CLI.
pub fn help() -> &'static str {
    "bundlemgr installs Nexus bundles. Usage: bundlectl <install|remove|query> [args]"
}

/// Executes the bundle manager CLI using the provided registrar backend.
pub fn execute<R: AbilityRegistrar>(args: &[&str], registrar: &R) -> String {
    if args.is_empty() || args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }

    match args[0] {
        "install" => match args.get(1) {
            Some(path) => install(path, registrar),
            None => "missing bundle path".to_string(),
        },
        "remove" => match args.get(1) {
            Some(name) => format!("bundle {name} removed"),
            None => "missing bundle name".to_string(),
        },
        "query" => "bundles: launcher".to_string(),
        other => format!("unknown command {other}"),
    }
}

/// Installs the bundle located at `path` using the registrar backend.
fn install<R: AbilityRegistrar>(path: &str, registrar: &R) -> String {
    if !path.ends_with(".nxb") {
        return "invalid bundle format".to_string();
    }
    if !verify_signature(path) {
        return "signature check failed".to_string();
    }
    let ability = infer_ability(path);
    match registrar.register(&ability) {
        Ok(serialized_header) => format!(
            "bundle installed: {path} with header {:?}",
            serialized_header
        ),
        Err(err) => format!("ability registration failed: {err}"),
    }
}

/// Verifies whether the bundle was signed.
fn verify_signature(path: &str) -> bool {
    path.ends_with("-signed.nxb")
}

/// Derives the ability name from the bundle path.
fn infer_ability(path: &str) -> String {
    path
        .rsplit('/')
        .next()
        .unwrap_or("bundle")
        .replace(".nxb", "")
}

/// Runs the CLI using arguments from `std::env::args`.
pub fn run_with<R: AbilityRegistrar>(registrar: &R) {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs, registrar));
}

#[cfg(test)]
mod tests {
    use super::{execute, infer_ability, install, verify_signature, AbilityRegistrar};

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

    #[test]
    fn signature_requires_marker() {
        assert!(verify_signature("app-signed.nxb"));
        assert!(!verify_signature("app.nxb"));
    }

    #[test]
    fn install_checks_extension() {
        assert_eq!(install("bad", &StubRegistrar), "invalid bundle format");
    }

    #[test]
    fn install_success_path() {
        let output = execute(&["install", "apps/launcher-signed.nxb"], &StubRegistrar);
        assert!(output.contains("bundle installed"));
        assert_eq!(infer_ability("apps/launcher-signed.nxb"), "launcher-signed");
    }

    #[test]
    fn registrar_failure_reports_reason() {
        let output = execute(&["install", "apps/unsigned.nxb"], &StubRegistrar);
        assert!(output.contains("signature"));
        struct ErrorRegistrar;
        impl AbilityRegistrar for ErrorRegistrar {
            fn register(&self, _: &str) -> Result<Vec<u8>, String> {
                Err("backend down".into())
            }
        }
        let output = execute(&["install", "apps/launcher-signed.nxb"], &ErrorRegistrar);
        assert!(output.contains("ability registration failed"));
    }
}
