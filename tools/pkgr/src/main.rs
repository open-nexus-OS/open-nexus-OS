use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn help() -> &'static str {
    "pkgr usage:\n  pkgr pack <app_dir>\n  pkgr sign <bundle.nxb>"
}

fn pack(path: &Path) -> io::Result<PathBuf> {
    let bundle_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bundle");
    let output = path.with_file_name(format!("{bundle_name}.nxb"));
    let mut file = File::create(&output)?;
    writeln!(file, "NEXUS-BUNDLE {}", bundle_name)?;
    Ok(output)
}

fn sign(bundle: &Path) -> io::Result<PathBuf> {
    let sig_path = bundle.with_extension("nxb.sig");
    let mut file = File::create(&sig_path)?;
    writeln!(file, "SIGNATURE DUMMY")?;
    Ok(sig_path)
}

fn main() {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| {
        println!("{}", help());
        std::process::exit(0);
    });

    match command.as_str() {
        "pack" => {
            let path = args.next().expect("missing path");
            match pack(Path::new(&path)) {
                Ok(bundle) => println!("packed {:?}", bundle),
                Err(err) => {
                    eprintln!("pack failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        "sign" => {
            let path = args.next().expect("missing bundle");
            match sign(Path::new(&path)) {
                Ok(sig) => println!("signed {:?}", sig),
                Err(err) => {
                    eprintln!("sign failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        "--help" | "-h" => println!("{}", help()),
        other => {
            eprintln!("unknown command {other}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pack, sign};
    use std::fs;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("pkgr-test-{name}"));
        path
    }

    #[test]
    fn pack_creates_file() {
        let dir = temp_path("pack");
        fs::create_dir_all(&dir).unwrap();
        let bundle = pack(&dir).expect("pack");
        assert!(bundle.exists());
        let _ = fs::remove_file(&bundle);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sign_creates_signature() {
        let bundle = temp_path("signed.nxb");
        fs::write(&bundle, b"dummy").unwrap();
        let sig = sign(&bundle).expect("sign");
        assert!(sig.exists());
        let _ = fs::remove_file(&bundle);
        let _ = fs::remove_file(&sig);
    }
}
