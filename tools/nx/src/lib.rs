// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Canonical host-first `nx` CLI contract implementation, including deterministic `nx config` behavior.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 17 unit tests in this module plus 6 process-boundary integration tests in `tests/cli_contract.rs`.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use clap::{Args, Parser, Subcommand, ValueEnum};
use configd::{Configd, ReloadReport};
use nexus_config::{
    build_effective_snapshot, env_overrides_from_pairs, load_config_path, load_layer_dir,
    LayerInputs, STATE_CONFIG_FILENAME,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const CARGO_TOML_TEMPLATE: &str = include_str!("../templates/Cargo.toml.tpl");
const MAIN_RS_TEMPLATE: &str = include_str!("../templates/main.rs.tpl");
const STUB_README_TEMPLATE: &str = include_str!("../templates/stub-readme.md.tpl");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    Success,
    Usage,
    ValidationReject,
    MissingDependency,
    DelegateFailure,
    Unsupported,
    Internal,
}

impl ExitClass {
    pub fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Usage => 2,
            Self::ValidationReject => 3,
            Self::MissingDependency => 4,
            Self::DelegateFailure => 5,
            Self::Unsupported => 6,
            Self::Internal => 7,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Usage => "usage",
            Self::ValidationReject => "validation_reject",
            Self::MissingDependency => "missing_dependency",
            Self::DelegateFailure => "delegate_failure",
            Self::Unsupported => "unsupported",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug)]
pub struct NxError {
    class: ExitClass,
    message: String,
}

impl NxError {
    fn new(class: ExitClass, message: impl Into<String>) -> Self {
        Self { class, message: message.into() }
    }
}

impl fmt::Display for NxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NxError {}

#[derive(Parser, Debug)]
#[command(name = "nx")]
#[command(about = "Open Nexus host CLI (v1 production-floor)")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    fn wants_json(&self) -> bool {
        match &self.command {
            Commands::New(args) => match &args.kind {
                NewKind::Service(a) | NewKind::App(a) | NewKind::Test(a) => a.json,
            },
            Commands::Inspect(args) => match &args.target {
                InspectTarget::Nxb(a) => a.json,
            },
            Commands::Idl(args) => match &args.action {
                IdlAction::List(a) => a.json,
                IdlAction::Check(a) => a.json,
            },
            Commands::Postflight(args) => args.json,
            Commands::Doctor(args) => args.json,
            Commands::Dsl(args) => args.json,
            Commands::Config(args) => match &args.action {
                ConfigAction::Validate(a) => a.json,
                ConfigAction::Effective(a) => a.json,
                ConfigAction::Diff(a) => a.json,
                ConfigAction::Push(a) => a.json,
                ConfigAction::Reload(a) => a.json,
                ConfigAction::Where(a) => a.json,
            },
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    New(NewArgs),
    Inspect(InspectArgs),
    Idl(IdlArgs),
    Postflight(PostflightArgs),
    Doctor(DoctorArgs),
    Dsl(DslArgs),
    Config(ConfigArgs),
}

#[derive(Args, Debug)]
struct NewArgs {
    #[command(subcommand)]
    kind: NewKind,
}

#[derive(Subcommand, Debug)]
enum NewKind {
    Service(NewItemArgs),
    App(NewItemArgs),
    Test(NewItemArgs),
}

#[derive(Args, Debug)]
struct NewItemArgs {
    name: String,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct InspectArgs {
    #[command(subcommand)]
    target: InspectTarget,
}

#[derive(Subcommand, Debug)]
enum InspectTarget {
    Nxb(InspectNxbArgs),
}

#[derive(Args, Debug)]
struct InspectNxbArgs {
    path: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct IdlArgs {
    #[command(subcommand)]
    action: IdlAction,
}

#[derive(Subcommand, Debug)]
enum IdlAction {
    List(IdlListArgs),
    Check(IdlCheckArgs),
}

#[derive(Args, Debug)]
struct IdlListArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct IdlCheckArgs {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct PostflightArgs {
    topic: String,
    #[arg(long, default_value_t = 40)]
    tail: usize,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct DoctorArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct DslArgs {
    action: DslAction,
    #[arg(last = true)]
    args: Vec<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ConfigArgs {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    Validate(ConfigValidateArgs),
    Effective(ConfigEffectiveArgs),
    Diff(ConfigDiffArgs),
    Push(ConfigPushArgs),
    Reload(ConfigReloadArgs),
    Where(ConfigWhereArgs),
}

#[derive(Args, Debug)]
struct ConfigValidateArgs {
    paths: Vec<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ConfigEffectiveArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ConfigDiffArgs {
    #[arg(long)]
    from: PathBuf,
    #[arg(long)]
    to: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ConfigPushArgs {
    file: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ConfigReloadArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct ConfigWhereArgs {
    #[arg(long)]
    json: bool,
}

#[derive(ValueEnum, Clone, Debug)]
enum DslAction {
    Fmt,
    Lint,
    Build,
}

#[derive(Debug, Clone)]
struct RuntimeConfig {
    repo_root: PathBuf,
    postflight_dir: PathBuf,
    dsl_backend: Option<PathBuf>,
}

impl RuntimeConfig {
    fn from_env() -> Result<Self, NxError> {
        let repo_root = std::env::current_dir().map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed to resolve cwd: {e}"))
        })?;
        Ok(Self {
            postflight_dir: repo_root.join("tools"),
            dsl_backend: std::env::var_os("NX_DSL_BACKEND").map(PathBuf::from),
            repo_root,
        })
    }
}

#[derive(Serialize)]
struct OutputEnvelope {
    ok: bool,
    class: &'static str,
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

fn print_result(class: ExitClass, message: String, json_mode: bool, data: Option<Value>) {
    if json_mode {
        let payload = OutputEnvelope {
            ok: class == ExitClass::Success,
            class: class.label(),
            code: class.code(),
            message,
            data,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{\"ok\":false}".to_string())
        );
        return;
    }

    println!("{message}");
    if let Some(data) = data {
        println!("{}", serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_string()));
    }
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    let wants_json = cli.wants_json();
    let cfg = match RuntimeConfig::from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            print_result(err.class, err.message, wants_json, None);
            return err.class.code();
        }
    };
    match execute(cli, &cfg) {
        Ok((class, message, json_mode, data)) => {
            print_result(class, message, json_mode, data);
            class.code()
        }
        Err(err) => {
            print_result(err.class, err.message, wants_json, None);
            err.class.code()
        }
    }
}

type ExecResult = Result<(ExitClass, String, bool, Option<Value>), NxError>;

fn execute(cli: Cli, cfg: &RuntimeConfig) -> ExecResult {
    match cli.command {
        Commands::New(args) => handle_new(args, cfg),
        Commands::Inspect(args) => handle_inspect(args),
        Commands::Idl(args) => handle_idl(args, cfg),
        Commands::Postflight(args) => handle_postflight(args, cfg),
        Commands::Doctor(args) => handle_doctor(args),
        Commands::Dsl(args) => handle_dsl(args, cfg),
        Commands::Config(args) => handle_config(args, cfg),
    }
}

fn validate_name(name: &str) -> Result<(), NxError> {
    if name.is_empty() {
        return Err(NxError::new(ExitClass::ValidationReject, "name must not be empty"));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "name rejects traversal or path separators",
        ));
    }
    Ok(())
}

fn validate_relative_root(root: &Path) -> Result<(), NxError> {
    if root.is_absolute() {
        return Err(NxError::new(ExitClass::ValidationReject, "absolute root path is rejected"));
    }
    if root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(NxError::new(ExitClass::ValidationReject, "root path traversal is rejected"));
    }
    Ok(())
}

fn handle_new(args: NewArgs, cfg: &RuntimeConfig) -> ExecResult {
    let (kind, item_args, base_path, template_title) = match args.kind {
        NewKind::Service(a) => ("service", a, Path::new("source/services"), "service"),
        NewKind::App(a) => ("app", a, Path::new("userspace/apps"), "app"),
        NewKind::Test(a) => ("test", a, Path::new("tests"), "test"),
    };

    validate_name(&item_args.name)?;
    if let Some(root) = &item_args.root {
        validate_relative_root(root)?;
    }
    let root = cfg.repo_root.join(item_args.root.as_deref().unwrap_or(Path::new(".")));
    let target_name =
        if kind == "test" { format!("{}_host", item_args.name) } else { item_args.name.clone() };
    let target_dir = root.join(base_path).join(&target_name);

    if target_dir.exists() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("target already exists: {}", target_dir.display()),
        ));
    }

    fs::create_dir_all(target_dir.join("src"))
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed creating tree: {e}")))?;
    fs::create_dir_all(target_dir.join("docs/stubs")).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("failed creating docs tree: {e}"))
    })?;

    let cargo_toml = CARGO_TOML_TEMPLATE.replace("{{CRATE_NAME}}", &target_name.replace('-', "_"));
    let main_rs = MAIN_RS_TEMPLATE.to_string();
    let stub_doc = STUB_README_TEMPLATE.replace("{{KIND}}", template_title);

    fs::write(target_dir.join("Cargo.toml"), cargo_toml).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("failed writing Cargo.toml: {e}"))
    })?;
    fs::write(target_dir.join("src/main.rs"), main_rs)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed writing main.rs: {e}")))?;
    fs::write(target_dir.join("docs/stubs/README.md"), stub_doc).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("failed writing stub README: {e}"))
    })?;

    let message = format!(
        "created {kind} scaffold at {}; workspace manifest not edited; add member manually",
        target_dir.display()
    );
    let data = json!({
        "kind": kind,
        "target": target_dir,
        "next_step": "manually register workspace member"
    });
    Ok((ExitClass::Success, message, item_args.json, Some(data)))
}

fn handle_inspect(args: InspectArgs) -> ExecResult {
    match args.target {
        InspectTarget::Nxb(nxb) => handle_inspect_nxb(nxb),
    }
}

fn handle_inspect_nxb(args: InspectNxbArgs) -> ExecResult {
    if !args.path.exists() || !args.path.is_dir() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "inspect nxb requires an existing directory",
        ));
    }

    let mut manifest_files = Vec::new();
    let mut meta_files = Vec::new();
    let mut payload_sha256 = None;

    let entries = fs::read_dir(&args.path)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed to read directory: {e}")))?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed to iterate directory: {e}"))
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("manifest.") {
            manifest_files.push(name.to_string());
        }
    }
    manifest_files.sort();

    let payload_path = args.path.join("payload.elf");
    if payload_path.exists() {
        let mut file = fs::File::open(&payload_path).map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed opening payload.elf: {e}"))
        })?;
        let mut hasher = Sha256::new();
        let mut buf = [0_u8; 8192];
        loop {
            let read = file.read(&mut buf).map_err(|e| {
                NxError::new(ExitClass::Internal, format!("failed reading payload.elf: {e}"))
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        payload_sha256 = Some(format!("{:x}", hasher.finalize()));
    }

    let meta_dir = args.path.join("meta");
    if meta_dir.exists() && meta_dir.is_dir() {
        collect_files(&meta_dir, &mut meta_files, &meta_dir)?;
        meta_files.sort();
    }

    let data = json!({
        "path": args.path,
        "manifest_files": manifest_files,
        "payload_present": payload_path.exists(),
        "payload_sha256": payload_sha256,
        "meta_files": meta_files,
    });
    Ok((ExitClass::Success, "inspect nxb summary generated".to_string(), args.json, Some(data)))
}

fn collect_files(root: &Path, out: &mut Vec<String>, strip_prefix: &Path) -> Result<(), NxError> {
    for entry in fs::read_dir(root)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed reading meta dir: {e}")))?
    {
        let entry = entry.map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed iterating meta dir: {e}"))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out, strip_prefix)?;
        } else {
            let rel = path.strip_prefix(strip_prefix).map_err(|e| {
                NxError::new(ExitClass::Internal, format!("failed strip prefix: {e}"))
            })?;
            out.push(rel.display().to_string());
        }
    }
    Ok(())
}

fn idl_root(cfg: &RuntimeConfig, root: Option<PathBuf>) -> PathBuf {
    match root {
        Some(p) => cfg.repo_root.join(p),
        None => cfg.repo_root.join("tools/nexus-idl/schemas"),
    }
}

fn handle_idl(args: IdlArgs, cfg: &RuntimeConfig) -> ExecResult {
    match args.action {
        IdlAction::List(list) => {
            let root = idl_root(cfg, list.root);
            let schemas = list_schemas(&root)?;
            let data = json!({
                "root": root,
                "schemas": schemas
            });
            Ok((
                ExitClass::Success,
                format!(
                    "listed {} schema file(s)",
                    data["schemas"].as_array().map(|v| v.len()).unwrap_or(0)
                ),
                list.json,
                Some(data),
            ))
        }
        IdlAction::Check(check) => {
            let root = idl_root(cfg, check.root);
            let schemas = list_schemas(&root)?;
            let capnp_ok = which("capnp").is_some();
            if !capnp_ok {
                return Err(NxError::new(
                    ExitClass::MissingDependency,
                    "required tool missing: capnp",
                ));
            }
            let data = json!({
                "root": root,
                "schema_count": schemas.len(),
                "capnp": capnp_ok,
            });
            Ok((ExitClass::Success, "idl check passed".to_string(), check.json, Some(data)))
        }
    }
}

fn list_schemas(root: &Path) -> Result<Vec<String>, NxError> {
    if !root.exists() || !root.is_dir() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("idl root does not exist: {}", root.display()),
        ));
    }
    let mut schemas = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed reading idl root: {e}")))?
    {
        let entry = entry.map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed iterating idl root: {e}"))
        })?;
        let path = entry.path();
        if path.extension() == Some(OsStr::new("capnp")) {
            schemas.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    schemas.sort();
    if schemas.is_empty() {
        return Err(NxError::new(ExitClass::ValidationReject, "no schema files found in idl root"));
    }
    Ok(schemas)
}

fn postflight_topics() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("kspawn", "postflight-kspawn.sh"),
        ("loader", "postflight-loader.sh"),
        ("loader-v1_1", "postflight-loader-v1_1.sh"),
        ("min-exec", "postflight-min-exec.sh"),
        ("policy", "postflight-policy.sh"),
        ("proc", "postflight-proc.sh"),
        ("vfs", "postflight-vfs.sh"),
        ("vfs-userspace", "postflight-vfs-userspace.sh"),
    ])
}

fn handle_postflight(args: PostflightArgs, cfg: &RuntimeConfig) -> ExecResult {
    let topics = postflight_topics();
    let Some(script_name) = topics.get(args.topic.as_str()) else {
        let valid_topics = topics.keys().copied().collect::<Vec<_>>();
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!(
                "unknown postflight topic '{}'; valid topics: {}",
                args.topic,
                valid_topics.join(", ")
            ),
        ));
    };
    let script_path = cfg.postflight_dir.join(script_name);
    if !script_path.exists() {
        return Err(NxError::new(
            ExitClass::Unsupported,
            format!("postflight script not available: {}", script_path.display()),
        ));
    }

    let start = Instant::now();
    let output = Command::new(&script_path).output().map_err(|e| {
        NxError::new(
            ExitClass::DelegateFailure,
            format!("failed to execute delegate {}: {e}", script_path.display()),
        )
    })?;
    let elapsed_ms = start.elapsed().as_millis();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail = bounded_tail(&[stdout.as_ref(), stderr.as_ref()].join("\n"), args.tail);

    let data = json!({
        "topic": args.topic,
        "script": script_path,
        "delegate_exit": output.status.code().unwrap_or(-1),
        "elapsed_ms": elapsed_ms,
        "tail": tail,
    });

    if output.status.success() {
        Ok((ExitClass::Success, "postflight delegate succeeded".to_string(), args.json, Some(data)))
    } else {
        Ok((
            ExitClass::DelegateFailure,
            "postflight delegate failed".to_string(),
            args.json,
            Some(data),
        ))
    }
}

fn bounded_tail(input: &str, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let lines = input.lines().map(ToString::to_string).collect::<Vec<_>>();
    let len = lines.len();
    if len <= max_lines {
        return lines;
    }
    lines[len - max_lines..].to_vec()
}

fn handle_doctor(args: DoctorArgs) -> ExecResult {
    handle_doctor_with_path(args, std::env::var_os("PATH"))
}

fn handle_doctor_with_path(args: DoctorArgs, path_var: Option<std::ffi::OsString>) -> ExecResult {
    let required = ["rustc", "cargo", "just", "qemu-system-riscv64", "capnp"];
    let optional = ["rg", "python3"];

    let mut missing_required = Vec::new();
    let mut found = serde_json::Map::new();

    for tool in required {
        let found_path = which_in_path(tool, path_var.as_deref());
        if found_path.is_none() {
            missing_required.push(tool.to_string());
        }
        found.insert(
            tool.to_string(),
            json!({
                "required": true,
                "found": found_path.is_some(),
                "path": found_path,
            }),
        );
    }
    for tool in optional {
        let found_path = which_in_path(tool, path_var.as_deref());
        found.insert(
            tool.to_string(),
            json!({
                "required": false,
                "found": found_path.is_some(),
                "path": found_path,
            }),
        );
    }

    let data = json!({
        "tools": found,
        "missing_required": missing_required,
        "hint": "Install missing required tools and rerun nx doctor"
    });

    if data["missing_required"].as_array().map(|v| v.is_empty()).unwrap_or(false) {
        Ok((ExitClass::Success, "doctor passed".to_string(), args.json, Some(data)))
    } else {
        Ok((
            ExitClass::MissingDependency,
            "doctor detected missing required tools".to_string(),
            args.json,
            Some(data),
        ))
    }
}

fn which(bin: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    which_in_path(bin, Some(&paths))
}

fn which_in_path(bin: &str, path_var: Option<&OsStr>) -> Option<String> {
    let paths = path_var?;
    for path in std::env::split_paths(&paths) {
        let full = path.join(bin);
        if full.is_file() {
            return Some(full.display().to_string());
        }
    }
    None
}

fn handle_dsl(args: DslArgs, cfg: &RuntimeConfig) -> ExecResult {
    let backend = match &cfg.dsl_backend {
        Some(path) => path.clone(),
        None => {
            return Ok((
                ExitClass::Unsupported,
                "dsl backend unsupported; set NX_DSL_BACKEND to enable delegation".to_string(),
                args.json,
                Some(json!({
                    "action": format!("{:?}", args.action).to_lowercase(),
                    "classification": "unsupported",
                })),
            ));
        }
    };

    if !backend.exists() {
        return Ok((
            ExitClass::Unsupported,
            format!("dsl backend unsupported; backend not found: {}", backend.display()),
            args.json,
            Some(json!({
                "backend": backend,
                "classification": "unsupported",
            })),
        ));
    }

    let action = match args.action {
        DslAction::Fmt => "fmt",
        DslAction::Lint => "lint",
        DslAction::Build => "build",
    };
    let output = Command::new(&backend).arg(action).args(&args.args).output().map_err(|e| {
        NxError::new(ExitClass::DelegateFailure, format!("failed executing dsl delegate: {e}"))
    })?;

    let data = json!({
        "backend": backend,
        "action": action,
        "delegate_exit": output.status.code().unwrap_or(-1),
        "tail": bounded_tail(&String::from_utf8_lossy(&output.stderr), 40),
    });

    if output.status.success() {
        Ok((ExitClass::Success, "dsl delegate succeeded".to_string(), args.json, Some(data)))
    } else {
        Ok((ExitClass::DelegateFailure, "dsl delegate failed".to_string(), args.json, Some(data)))
    }
}

fn handle_config(args: ConfigArgs, cfg: &RuntimeConfig) -> ExecResult {
    match args.action {
        ConfigAction::Validate(a) => handle_config_validate(a, cfg),
        ConfigAction::Effective(a) => handle_config_effective(a, cfg),
        ConfigAction::Diff(a) => handle_config_diff(a, cfg),
        ConfigAction::Push(a) => handle_config_push(a, cfg),
        ConfigAction::Reload(a) => handle_config_reload(a, cfg),
        ConfigAction::Where(a) => handle_config_where(a, cfg),
    }
}

fn handle_config_validate(args: ConfigValidateArgs, cfg: &RuntimeConfig) -> ExecResult {
    if args.paths.is_empty() {
        let _ = load_layers_from_repo(cfg)?;
        return Ok((
            ExitClass::Success,
            "config validate passed for layered sources".to_string(),
            args.json,
            None,
        ));
    }

    for path in &args.paths {
        let overlay = load_config_path(path)
            .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
        let mut layers = LayerInputs::with_defaults_only();
        layers.state = overlay;
        build_effective_snapshot(layers)
            .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    }

    Ok((
        ExitClass::Success,
        format!("config validate passed for {} file(s)", args.paths.len()),
        args.json,
        None,
    ))
}

fn handle_config_effective(args: ConfigEffectiveArgs, cfg: &RuntimeConfig) -> ExecResult {
    let layers = load_layers_from_repo(cfg)?;
    let snapshot = build_effective_snapshot(layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let data = json!({
        "version": snapshot.version,
        "effective": snapshot.merged_json,
    });
    Ok((ExitClass::Success, "effective config generated".to_string(), args.json, Some(data)))
}

fn handle_config_diff(args: ConfigDiffArgs, _cfg: &RuntimeConfig) -> ExecResult {
    let from_overlay = read_overlay_file(&args.from)?;
    let to_overlay = read_overlay_file(&args.to)?;

    let mut from_layers = LayerInputs::with_defaults_only();
    from_layers.state = from_overlay;
    let from_snapshot = build_effective_snapshot(from_layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let mut to_layers = LayerInputs::with_defaults_only();
    to_layers.state = to_overlay;
    let to_snapshot = build_effective_snapshot(to_layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let changed = from_snapshot.version != to_snapshot.version;
    let data = json!({
        "changed": changed,
        "from_version": from_snapshot.version,
        "to_version": to_snapshot.version,
        "from_effective": from_snapshot.merged_json,
        "to_effective": to_snapshot.merged_json,
    });
    Ok((ExitClass::Success, "config diff generated".to_string(), args.json, Some(data)))
}

fn handle_config_push(args: ConfigPushArgs, cfg: &RuntimeConfig) -> ExecResult {
    let bytes = fs::read(&args.file).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!("failed reading push source '{}': {e}", args.file.display()),
        )
    })?;
    let overlay = load_config_path(&args.file)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let mut layers = LayerInputs::with_defaults_only();
    layers.state = overlay;
    build_effective_snapshot(layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let state_dir = cfg.repo_root.join("state/config");
    fs::create_dir_all(&state_dir).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!("failed creating state config directory '{}': {e}", state_dir.display()),
        )
    })?;
    let state_path = state_dir.join(format!("{STATE_CONFIG_FILENAME}.json"));
    fs::write(&state_path, bytes).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!("failed writing state config '{}': {e}", state_path.display()),
        )
    })?;
    Ok((
        ExitClass::Success,
        format!("config pushed to {}", state_path.display()),
        args.json,
        Some(json!({ "path": state_path })),
    ))
}

fn handle_config_reload(args: ConfigReloadArgs, cfg: &RuntimeConfig) -> ExecResult {
    let layers = load_layers_from_repo(cfg)?;
    let mut daemon = Configd::new(layers.clone())
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let report: ReloadReport = daemon
        .reload(layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let class = if report.committed { ExitClass::Success } else { ExitClass::DelegateFailure };
    let message = if report.committed {
        "config reload committed".to_string()
    } else {
        "config reload aborted".to_string()
    };
    let data = json!({
        "committed": report.committed,
        "from_version": report.from_version,
        "candidate_version": report.candidate_version,
        "active_version": report.active_version,
        "reason": report.reason,
    });
    Ok((class, message, args.json, Some(data)))
}

fn handle_config_where(args: ConfigWhereArgs, cfg: &RuntimeConfig) -> ExecResult {
    let data = config_paths(cfg);
    Ok((ExitClass::Success, "config source paths".to_string(), args.json, Some(json!(data))))
}

fn config_paths(cfg: &RuntimeConfig) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("system".to_string(), cfg.repo_root.join("system/config").display().to_string()),
        ("state".to_string(), cfg.repo_root.join("state/config").display().to_string()),
        ("env_prefix".to_string(), "NEXUS_CFG_".to_string()),
    ])
}

fn load_layers_from_repo(cfg: &RuntimeConfig) -> Result<LayerInputs, NxError> {
    let mut layers = LayerInputs::with_defaults_only();
    let paths = config_paths(cfg);
    let system_path = PathBuf::from(
        paths
            .get("system")
            .ok_or_else(|| NxError::new(ExitClass::Internal, "missing system config path"))?,
    );
    let state_path = PathBuf::from(
        paths
            .get("state")
            .ok_or_else(|| NxError::new(ExitClass::Internal, "missing state config path"))?,
    );

    layers.system = load_layer_dir(&system_path)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    layers.state = load_layer_dir(&state_path)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let env_pairs =
        std::env::vars().filter(|(k, _)| k.starts_with("NEXUS_CFG_")).collect::<BTreeMap<_, _>>();
    layers.env = env_overrides_from_pairs(&env_pairs)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    Ok(layers)
}

fn read_overlay_file(path: &Path) -> Result<Value, NxError> {
    load_config_path(path).map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn test_cfg(root: &Path) -> RuntimeConfig {
        RuntimeConfig {
            repo_root: root.to_path_buf(),
            postflight_dir: root.join("tools"),
            dsl_backend: None,
        }
    }

    #[test]
    fn test_reject_new_service_path_traversal() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "new", "service", "../escape"]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("must reject traversal");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_reject_new_service_absolute_path() {
        let root = TempDir::new().expect("tempdir");
        let cli =
            Cli::parse_from(["nx", "new", "service", "svc", "--root", "/tmp/absolute-path-reject"]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("must reject absolute root");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_new_service_creates_expected_tree() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "new", "service", "svc-a", "--json"]);
        let (class, _, _, _) = execute(cli, &test_cfg(root.path())).expect("must succeed");
        assert_eq!(class, ExitClass::Success);
        assert!(root.path().join("source/services/svc-a/Cargo.toml").exists());
        assert!(root.path().join("source/services/svc-a/src/main.rs").exists());
    }

    #[test]
    fn test_reject_unknown_postflight_topic() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "postflight", "unknown-topic"]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("must reject unknown topic");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_postflight_failure_passthrough() {
        let root = TempDir::new().expect("tempdir");
        let tools = root.path().join("tools");
        fs::create_dir_all(&tools).expect("tools dir");
        let script = tools.join("postflight-vfs.sh");
        fs::write(&script, "#!/usr/bin/env sh\nexit 9\n").expect("write script");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("set perms");
        }

        let cli = Cli::parse_from(["nx", "postflight", "vfs", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("must run");
        assert_eq!(class, ExitClass::DelegateFailure);
        assert_eq!(data.expect("data")["delegate_exit"], json!(9));
    }

    #[test]
    fn test_postflight_success_passthrough() {
        let root = TempDir::new().expect("tempdir");
        let tools = root.path().join("tools");
        fs::create_dir_all(&tools).expect("tools dir");
        let script = tools.join("postflight-vfs.sh");
        fs::write(&script, "#!/usr/bin/env sh\nexit 0\n").expect("write script");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("set perms");
        }

        let cli = Cli::parse_from(["nx", "postflight", "vfs", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("must run");
        assert_eq!(class, ExitClass::Success);
        assert_eq!(data.expect("data")["delegate_exit"], json!(0));
    }

    #[test]
    fn test_doctor_exit_nonzero_when_required_missing() {
        let args = DoctorArgs { json: true };
        let result = handle_doctor_with_path(args, Some("".into())).expect("doctor result");
        assert_eq!(result.0, ExitClass::MissingDependency);
    }

    #[test]
    fn test_doctor_reports_missing_required_tools() {
        let args = DoctorArgs { json: true };
        let (_, _, _, data) =
            handle_doctor_with_path(args, Some("".into())).expect("doctor result");
        let data = data.expect("data");
        let missing = data["missing_required"].as_array().expect("missing array");
        assert!(missing.len() >= 5);
    }

    #[test]
    fn test_dsl_wrapper_fail_closed_when_backend_missing() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "dsl", "fmt", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("must classify");
        assert_eq!(class, ExitClass::Unsupported);
        assert_eq!(data.expect("data")["classification"], json!("unsupported"));
    }

    #[test]
    fn test_dsl_wrapper_propagates_delegate_failure() {
        let root = TempDir::new().expect("tempdir");
        let backend = root.path().join("dsl-backend.sh");
        fs::write(&backend, "#!/usr/bin/env sh\nexit 4\n").expect("write backend");
        let mut perms = fs::metadata(&backend).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&backend, perms).expect("set perms");
        }

        let cli = Cli::parse_from(["nx", "dsl", "build", "--json"]);
        let mut cfg = test_cfg(root.path());
        cfg.dsl_backend = Some(backend);
        let (class, _, _, data) = execute(cli, &cfg).expect("must run");
        assert_eq!(class, ExitClass::DelegateFailure);
        assert_eq!(data.expect("data")["delegate_exit"], json!(4));
    }

    #[test]
    fn test_inspect_nxb_json_stable_fixture() {
        let root = TempDir::new().expect("tempdir");
        let nxb_dir = root.path().join("fixture.nxb");
        fs::create_dir_all(nxb_dir.join("meta")).expect("meta dir");
        fs::write(nxb_dir.join("manifest.toml"), "name = 'demo'\n").expect("manifest");
        fs::write(nxb_dir.join("payload.elf"), b"abc").expect("payload");
        fs::write(nxb_dir.join("meta/info.txt"), "ok").expect("meta file");

        let cli =
            Cli::parse_from(["nx", "inspect", "nxb", nxb_dir.to_string_lossy().as_ref(), "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("inspect works");
        assert_eq!(class, ExitClass::Success);
        let data = data.expect("data");
        assert_eq!(data["payload_present"], json!(true));
        assert!(data["payload_sha256"].is_string());
    }

    #[test]
    fn test_config_validate_rejects_unknown_field() {
        let root = TempDir::new().expect("tempdir");
        let input = root.path().join("bad-config.json");
        fs::write(
            &input,
            r#"{
  "dsoftbus": { "transport": "auto", "max_peers": 20, "unknown_knob": true }
}"#,
        )
        .expect("write");
        let cli = Cli::parse_from([
            "nx",
            "config",
            "validate",
            input.to_string_lossy().as_ref(),
            "--json",
        ]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("validation must fail");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_config_push_writes_state_config() {
        let root = TempDir::new().expect("tempdir");
        let input = root.path().join("good-config.json");
        fs::write(
            &input,
            r#"{
  "metrics": { "enabled": false, "flush_interval_ms": 1200 }
}"#,
        )
        .expect("write");
        let cli =
            Cli::parse_from(["nx", "config", "push", input.to_string_lossy().as_ref(), "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("push success");
        assert_eq!(class, ExitClass::Success);
        assert!(root.path().join("state/config/90-nx-config.json").exists());
        assert!(data.expect("data")["path"].is_string());
    }

    #[test]
    fn test_config_effective_is_deterministic() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("state/config")).expect("state dir");
        fs::write(
            root.path().join("state/config/90-nx-config.json"),
            r#"{"tracing":{"level":"debug"}}"#,
        )
        .expect("write state");

        let cli_a = Cli::parse_from(["nx", "config", "effective", "--json"]);
        let (_, _, _, data_a) = execute(cli_a, &test_cfg(root.path())).expect("effective a");
        let cli_b = Cli::parse_from(["nx", "config", "effective", "--json"]);
        let (_, _, _, data_b) = execute(cli_b, &test_cfg(root.path())).expect("effective b");
        assert_eq!(data_a, data_b);
    }

    #[test]
    fn test_config_effective_matches_configd_version_and_json() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("system/config")).expect("system dir");
        fs::create_dir_all(root.path().join("state/config")).expect("state dir");
        fs::write(
            root.path().join("system/config/10-base.json"),
            r#"{"metrics":{"enabled":true,"flush_interval_ms":2500}}"#,
        )
        .expect("write system");
        fs::write(
            root.path().join("state/config/90-nx-config.json"),
            r#"{"metrics":{"enabled":false},"tracing":{"level":"debug"}}"#,
        )
        .expect("write state");

        let cfg = test_cfg(root.path());
        let cli = Cli::parse_from(["nx", "config", "effective", "--json"]);
        let (_, _, _, data) = execute(cli, &cfg).expect("effective success");
        let data = data.expect("json data");

        let layers = load_layers_from_repo(&cfg).expect("load repo layers");
        let daemon = Configd::new(layers).expect("configd init");
        let daemon_view = daemon.get_effective_json();

        assert_eq!(data["version"], Value::String(daemon_view.version));
        assert_eq!(data["effective"], daemon_view.derived_json);
    }

    #[test]
    fn test_config_reload_reports_commit_and_active_version() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("state/config")).expect("state dir");
        fs::write(
            root.path().join("state/config/90-nx-config.json"),
            r#"{"metrics":{"enabled":false}}"#,
        )
        .expect("write state");

        let cli = Cli::parse_from(["nx", "config", "reload", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("reload success");
        let data = data.expect("json data");

        assert_eq!(class, ExitClass::Success);
        assert_eq!(data["committed"], Value::Bool(true));
        assert_eq!(data["candidate_version"], data["active_version"]);
    }

    #[test]
    fn test_config_where_returns_layer_directories() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "config", "where", "--json"]);
        let (_, _, _, data) = execute(cli, &test_cfg(root.path())).expect("where success");
        let data = data.expect("json data");

        assert_eq!(
            data["state"],
            Value::String(root.path().join("state/config").display().to_string())
        );
        assert_eq!(
            data["system"],
            Value::String(root.path().join("system/config").display().to_string())
        );
        assert_eq!(data["env_prefix"], Value::String("NEXUS_CFG_".to_string()));
    }
}
