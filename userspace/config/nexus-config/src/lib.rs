// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Config v1 deterministic layering, bounded validation, and canonical snapshot encoding.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 10 unit tests covering reject paths, deterministic layering, and Cap'n Proto snapshot determinism.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

#![deny(unsafe_code)]

use capnp::message::{Builder, ReaderOptions};
use capnp::serialize_packed;
use jsonschema::{validator_for, Validator};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
pub const MAX_CONFIG_DEPTH: usize = 16;
pub const MAX_LIST_LENGTH: usize = 128;
pub const STATE_CONFIG_FILENAME: &str = "90-nx-config";

const DSOFTBUS_SCHEMA_JSON: &str = include_str!("../../../../schemas/dsoftbus.schema.json");
const METRICS_SCHEMA_JSON: &str = include_str!("../../../../schemas/metrics.schema.json");
const TRACING_SCHEMA_JSON: &str = include_str!("../../../../schemas/tracing.schema.json");
const SECURITY_SANDBOX_SCHEMA_JSON: &str =
    include_str!("../../../../schemas/security.sandbox.schema.json");
const SCHED_SCHEMA_JSON: &str = include_str!("../../../../schemas/sched.schema.json");
const POLICY_SCHEMA_JSON: &str = include_str!("../../../../schemas/policy.config.schema.json");

#[allow(unsafe_code, clippy::unwrap_used, clippy::needless_lifetimes)]
pub mod config_effective_capnp {
    include!(concat!(env!("OUT_DIR"), "/config_effective_capnp.rs"));
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveConfig {
    pub dsoftbus: DsoftbusConfig,
    pub metrics: MetricsConfig,
    pub tracing: TracingConfig,
    pub security_sandbox: SecuritySandboxConfig,
    pub sched: SchedConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsoftbusConfig {
    pub transport: String,
    pub max_peers: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub flush_interval_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TracingConfig {
    pub level: String,
    pub sample_rate_per_mille: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecuritySandboxConfig {
    pub default_profile: String,
    pub max_caps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedConfig {
    pub default_qos: String,
    pub runqueue_slice_ms: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub root: String,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            root: "policies".to_string(),
        }
    }
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            dsoftbus: DsoftbusConfig {
                transport: "auto".to_string(),
                max_peers: 256,
            },
            metrics: MetricsConfig {
                enabled: true,
                flush_interval_ms: 1000,
            },
            tracing: TracingConfig {
                level: "info".to_string(),
                sample_rate_per_mille: 100,
            },
            security_sandbox: SecuritySandboxConfig {
                default_profile: "base".to_string(),
                max_caps: 64,
            },
            sched: SchedConfig {
                default_qos: "normal".to_string(),
                runqueue_slice_ms: 10,
            },
            policy: PolicyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerInputs {
    pub defaults: Value,
    pub system: Value,
    pub state: Value,
    pub env: Value,
}

impl LayerInputs {
    pub fn with_defaults_only() -> Self {
        Self {
            defaults: serde_json::to_value(EffectiveConfig::default())
                .unwrap_or(Value::Object(Map::new())),
            system: Value::Object(Map::new()),
            state: Value::Object(Map::new()),
            env: Value::Object(Map::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSnapshot {
    pub effective: EffectiveConfig,
    pub merged_json: Value,
    pub capnp_bytes: Vec<u8>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectClass {
    UnknownField,
    TypeMismatch,
    DepthOverflow,
    SizeOverflow,
    ListOverflow,
    Serialization,
}

impl RejectClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnknownField => "reject.unknown_field",
            Self::TypeMismatch => "reject.type_mismatch",
            Self::DepthOverflow => "reject.depth_overflow",
            Self::SizeOverflow => "reject.size_overflow",
            Self::ListOverflow => "reject.list_overflow",
            Self::Serialization => "reject.serialization",
        }
    }
}

impl std::fmt::Display for RejectClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{class}: {detail}")]
    Reject { class: RejectClass, detail: String },
    #[error("failed to parse json source: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("failed to encode/decode capnp: {0}")]
    Capnp(String),
    #[error("failed to read config source: {0}")]
    Io(String),
    #[error("failed to compile schema: {0}")]
    Schema(String),
}

impl ConfigError {
    pub fn class(&self) -> Option<&RejectClass> {
        match self {
            Self::Reject { class, .. } => Some(class),
            _ => None,
        }
    }
}

pub fn load_json_source(bytes: &[u8]) -> Result<Value, ConfigError> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::Reject {
            class: RejectClass::SizeOverflow,
            detail: format!("source size {} exceeds {}", bytes.len(), MAX_CONFIG_BYTES),
        });
    }
    let value: Value = serde_json::from_slice(bytes)?;
    enforce_bounds(&value, "$", 0)?;
    Ok(value)
}

pub fn load_config_path(path: &Path) -> Result<Value, ConfigError> {
    let bytes = fs::read(path)
        .map_err(|e| ConfigError::Io(format!("failed reading '{}': {e}", path.display())))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => load_json_source(&bytes),
        Some(other) => Err(ConfigError::Reject {
            class: RejectClass::Serialization,
            detail: format!(
                "unsupported config extension '.{other}' for {}",
                path.display()
            ),
        }),
        None => Err(ConfigError::Reject {
            class: RejectClass::Serialization,
            detail: format!("missing config extension for {}", path.display()),
        }),
    }
}

pub fn load_layer_dir(dir: &Path) -> Result<Value, ConfigError> {
    if !dir.exists() {
        return Ok(Value::Object(Map::new()));
    }
    if !dir.is_dir() {
        return Err(ConfigError::Io(format!(
            "config layer path '{}' is not a directory",
            dir.display()
        )));
    }

    let mut entries = fs::read_dir(dir)
        .map_err(|e| ConfigError::Io(format!("failed reading layer dir '{}': {e}", dir.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            ConfigError::Io(format!(
                "failed iterating layer dir '{}': {e}",
                dir.display()
            ))
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    let mut merged = Value::Object(Map::new());
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => {
                let overlay = load_config_path(&path)?;
                deep_merge_replace_lists(&mut merged, &overlay);
            }
            _ => continue,
        }
    }
    Ok(merged)
}

pub fn env_overrides_from_pairs(pairs: &BTreeMap<String, String>) -> Result<Value, ConfigError> {
    let mut root = Map::new();
    for (key, raw_value) in pairs {
        if !key.starts_with("NEXUS_CFG_") {
            continue;
        }
        let suffix = key.trim_start_matches("NEXUS_CFG_");
        if suffix.is_empty() {
            continue;
        }
        let segments = suffix
            .split("__")
            .map(|s| s.to_ascii_lowercase())
            .collect::<Vec<_>>();
        if segments.iter().any(|s| s.is_empty()) {
            return Err(ConfigError::Reject {
                class: RejectClass::UnknownField,
                detail: format!("invalid env override key: {key}"),
            });
        }
        let parsed_value = parse_env_value(raw_value);
        apply_env_path(&mut root, &segments, parsed_value)?;
    }
    let value = Value::Object(root);
    enforce_bounds(&value, "$", 0)?;
    Ok(value)
}

pub fn build_effective_snapshot(inputs: LayerInputs) -> Result<EffectiveSnapshot, ConfigError> {
    for source in [&inputs.defaults, &inputs.system, &inputs.state, &inputs.env] {
        enforce_bounds(source, "$", 0)?;
    }
    let merged = merge_layers(&inputs.defaults, &inputs.system, &inputs.state, &inputs.env);
    enforce_bounds(&merged, "$", 0)?;
    validate_against_json_schemas(&merged)?;

    let effective: EffectiveConfig =
        serde_json::from_value(merged.clone()).map_err(classify_serde_validation_error)?;
    let merged = serde_json::to_value(&effective)?;
    let capnp_bytes = encode_capnp(&effective)?;
    let version = version_from_capnp_bytes(&capnp_bytes);

    Ok(EffectiveSnapshot {
        effective,
        merged_json: merged,
        capnp_bytes,
        version,
    })
}

pub fn version_from_capnp_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn decode_effective_capnp(bytes: &[u8]) -> Result<EffectiveConfig, ConfigError> {
    let mut slice = bytes;
    let message = serialize_packed::read_message(&mut slice, ReaderOptions::new())
        .map_err(|e| ConfigError::Capnp(format!("read_message failed: {e}")))?;
    let root = message
        .get_root::<config_effective_capnp::effective_config::Reader<'_>>()
        .map_err(|e| ConfigError::Capnp(format!("get_root failed: {e}")))?;

    let dsoftbus = root
        .get_dsoftbus()
        .map_err(|e| ConfigError::Capnp(format!("read dsoftbus failed: {e}")))?;
    let metrics = root
        .get_metrics()
        .map_err(|e| ConfigError::Capnp(format!("read metrics failed: {e}")))?;
    let tracing = root
        .get_tracing()
        .map_err(|e| ConfigError::Capnp(format!("read tracing failed: {e}")))?;
    let security = root
        .get_security_sandbox()
        .map_err(|e| ConfigError::Capnp(format!("read security_sandbox failed: {e}")))?;
    let sched = root
        .get_sched()
        .map_err(|e| ConfigError::Capnp(format!("read sched failed: {e}")))?;
    let policy = root
        .get_policy()
        .map_err(|e| ConfigError::Capnp(format!("read policy failed: {e}")))?;

    Ok(EffectiveConfig {
        dsoftbus: DsoftbusConfig {
            transport: dsoftbus
                .get_transport()
                .map_err(|e| ConfigError::Capnp(format!("read transport failed: {e}")))?
                .to_string()
                .map_err(|e| ConfigError::Capnp(format!("utf8 transport failed: {e}")))?,
            max_peers: dsoftbus.get_max_peers(),
        },
        metrics: MetricsConfig {
            enabled: metrics.get_enabled(),
            flush_interval_ms: metrics.get_flush_interval_ms(),
        },
        tracing: TracingConfig {
            level: tracing
                .get_level()
                .map_err(|e| ConfigError::Capnp(format!("read level failed: {e}")))?
                .to_string()
                .map_err(|e| ConfigError::Capnp(format!("utf8 level failed: {e}")))?,
            sample_rate_per_mille: tracing.get_sample_rate_permille(),
        },
        security_sandbox: SecuritySandboxConfig {
            default_profile: security
                .get_default_profile()
                .map_err(|e| ConfigError::Capnp(format!("read default_profile failed: {e}")))?
                .to_string()
                .map_err(|e| ConfigError::Capnp(format!("utf8 default_profile failed: {e}")))?,
            max_caps: security.get_max_caps(),
        },
        sched: SchedConfig {
            default_qos: sched
                .get_default_qos()
                .map_err(|e| ConfigError::Capnp(format!("read default_qos failed: {e}")))?
                .to_string()
                .map_err(|e| ConfigError::Capnp(format!("utf8 default_qos failed: {e}")))?,
            runqueue_slice_ms: sched.get_runqueue_slice_ms(),
        },
        policy: PolicyConfig {
            root: policy
                .get_root()
                .map_err(|e| ConfigError::Capnp(format!("read policy root failed: {e}")))?
                .to_string()
                .map_err(|e| ConfigError::Capnp(format!("utf8 policy root failed: {e}")))?,
        },
    })
}

fn encode_capnp(effective: &EffectiveConfig) -> Result<Vec<u8>, ConfigError> {
    let mut message = Builder::new_default();
    {
        let mut root = message.init_root::<config_effective_capnp::effective_config::Builder<'_>>();
        root.set_schema_version(1);

        let mut dsoftbus = root.reborrow().init_dsoftbus();
        dsoftbus.set_transport(&effective.dsoftbus.transport);
        dsoftbus.set_max_peers(effective.dsoftbus.max_peers);

        let mut metrics = root.reborrow().init_metrics();
        metrics.set_enabled(effective.metrics.enabled);
        metrics.set_flush_interval_ms(effective.metrics.flush_interval_ms);

        let mut tracing = root.reborrow().init_tracing();
        tracing.set_level(&effective.tracing.level);
        tracing.set_sample_rate_permille(effective.tracing.sample_rate_per_mille);

        let mut security = root.reborrow().init_security_sandbox();
        security.set_default_profile(&effective.security_sandbox.default_profile);
        security.set_max_caps(effective.security_sandbox.max_caps);

        let mut sched = root.reborrow().init_sched();
        sched.set_default_qos(&effective.sched.default_qos);
        sched.set_runqueue_slice_ms(effective.sched.runqueue_slice_ms);

        let mut policy = root.reborrow().init_policy();
        policy.set_root(&effective.policy.root);
    }

    let mut out = Vec::new();
    serialize_packed::write_message(&mut out, &message)
        .map_err(|e| ConfigError::Capnp(format!("write_message failed: {e}")))?;
    Ok(out)
}

fn parse_env_value(raw: &str) -> Value {
    if raw.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(num) = raw.parse::<i64>() {
        return Value::Number(num.into());
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
        return parsed;
    }
    Value::String(raw.to_string())
}

fn apply_env_path(
    root: &mut Map<String, Value>,
    segments: &[String],
    value: Value,
) -> Result<(), ConfigError> {
    let (leaf, parents) = segments.split_last().ok_or_else(|| ConfigError::Reject {
        class: RejectClass::UnknownField,
        detail: "empty env path".to_string(),
    })?;
    let mut cursor = root;
    for seg in parents {
        let entry = cursor
            .entry(seg.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        let object = entry.as_object_mut().ok_or_else(|| ConfigError::Reject {
            class: RejectClass::TypeMismatch,
            detail: format!("env path segment '{seg}' collides with non-object"),
        })?;
        cursor = object;
    }
    cursor.insert(leaf.clone(), value);
    Ok(())
}

fn merge_layers(defaults: &Value, system: &Value, state: &Value, env: &Value) -> Value {
    let mut merged = defaults.clone();
    deep_merge_replace_lists(&mut merged, system);
    deep_merge_replace_lists(&mut merged, state);
    deep_merge_replace_lists(&mut merged, env);
    merged
}

fn deep_merge_replace_lists(target: &mut Value, overlay: &Value) {
    match (target, overlay) {
        (Value::Object(target_obj), Value::Object(overlay_obj)) => {
            for (k, v) in overlay_obj {
                if let Some(target_value) = target_obj.get_mut(k) {
                    deep_merge_replace_lists(target_value, v);
                } else {
                    target_obj.insert(k.clone(), v.clone());
                }
            }
        }
        (target_slot, overlay_value) => {
            *target_slot = overlay_value.clone();
        }
    }
}

fn enforce_bounds(value: &Value, path: &str, depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_CONFIG_DEPTH {
        return Err(ConfigError::Reject {
            class: RejectClass::DepthOverflow,
            detail: format!("value at {path} exceeds max depth {MAX_CONFIG_DEPTH}"),
        });
    }
    match value {
        Value::Array(items) => {
            if items.len() > MAX_LIST_LENGTH {
                return Err(ConfigError::Reject {
                    class: RejectClass::ListOverflow,
                    detail: format!(
                        "array at {path} length {} exceeds max {MAX_LIST_LENGTH}",
                        items.len()
                    ),
                });
            }
            for (idx, item) in items.iter().enumerate() {
                enforce_bounds(item, &format!("{path}[{idx}]"), depth + 1)?;
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                enforce_bounds(item, &format!("{path}.{key}"), depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_against_json_schemas(merged: &Value) -> Result<(), ConfigError> {
    let object = merged.as_object().ok_or_else(|| ConfigError::Reject {
        class: RejectClass::TypeMismatch,
        detail: "effective config root must be an object".to_string(),
    })?;

    let schema_map = schema_validators()?;
    for key in object.keys() {
        if !schema_map.contains_key(key.as_str()) {
            return Err(ConfigError::Reject {
                class: RejectClass::UnknownField,
                detail: format!("unknown top-level config section '{key}'"),
            });
        }
    }

    for (section, validator) in &schema_map {
        if let Some(value) = object.get(*section) {
            if let Err(error) = validator.validate(value) {
                return Err(classify_schema_error(section, &error.to_string()));
            }
        }
    }
    Ok(())
}

fn schema_validators() -> Result<BTreeMap<&'static str, Validator>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "dsoftbus",
            compile_schema("dsoftbus", DSOFTBUS_SCHEMA_JSON)?,
        ),
        ("metrics", compile_schema("metrics", METRICS_SCHEMA_JSON)?),
        ("tracing", compile_schema("tracing", TRACING_SCHEMA_JSON)?),
        (
            "security_sandbox",
            compile_schema("security.sandbox", SECURITY_SANDBOX_SCHEMA_JSON)?,
        ),
        ("sched", compile_schema("sched", SCHED_SCHEMA_JSON)?),
        ("policy", compile_schema("policy", POLICY_SCHEMA_JSON)?),
    ]))
}

fn compile_schema(name: &'static str, contents: &'static str) -> Result<Validator, ConfigError> {
    let schema: Value = serde_json::from_str(contents)?;
    validator_for(&schema).map_err(|e| ConfigError::Schema(format!("{name}: {e}")))
}

fn classify_schema_error(section: &str, error: &str) -> ConfigError {
    let normalized = error.to_ascii_lowercase();
    let class = if normalized.contains("additionalproperties")
        || normalized.contains("additional properties")
        || normalized.contains("additional property")
    {
        RejectClass::UnknownField
    } else if normalized.contains("is not of type")
        || normalized.contains("\"type\"")
        || normalized.contains("is not one of")
        || normalized.contains("maximum")
        || normalized.contains("minimum")
        || normalized.contains("enum")
    {
        RejectClass::TypeMismatch
    } else {
        RejectClass::Serialization
    };
    ConfigError::Reject {
        class,
        detail: format!("schema validation failed for {section}: {error}"),
    }
}

fn classify_serde_validation_error(err: serde_json::Error) -> ConfigError {
    let msg = err.to_string();
    if msg.contains("unknown field") {
        return ConfigError::Reject {
            class: RejectClass::UnknownField,
            detail: msg,
        };
    }
    if msg.contains("invalid type") {
        return ConfigError::Reject {
            class: RejectClass::TypeMismatch,
            detail: msg,
        };
    }
    ConfigError::Reject {
        class: RejectClass::Serialization,
        detail: msg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_inputs() -> LayerInputs {
        LayerInputs {
            defaults: serde_json::json!({
                "dsoftbus": { "transport": "auto", "max_peers": 64 },
                "metrics": { "enabled": true, "flush_interval_ms": 2000 },
                "tracing": { "level": "info", "sample_rate_per_mille": 100 },
                "security_sandbox": { "default_profile": "base", "max_caps": 64 },
                "sched": { "default_qos": "normal", "runqueue_slice_ms": 15 }
            }),
            system: serde_json::json!({}),
            state: serde_json::json!({}),
            env: serde_json::json!({}),
        }
    }

    #[test]
    fn test_reject_config_unknown_field() {
        let mut inputs = sample_inputs();
        inputs.system = serde_json::json!({ "dsoftbus": { "unknown_knob": true } });
        let err = build_effective_snapshot(inputs).expect_err("unknown field must reject");
        assert_eq!(err.class().expect("class"), &RejectClass::UnknownField);
    }

    #[test]
    fn test_reject_config_type_mismatch() {
        let mut inputs = sample_inputs();
        inputs.state = serde_json::json!({ "metrics": { "enabled": "true" } });
        let err = build_effective_snapshot(inputs).expect_err("type mismatch must reject");
        assert_eq!(err.class().expect("class"), &RejectClass::TypeMismatch);
    }

    #[test]
    fn test_reject_config_depth_or_size_overflow() {
        let mut deep = serde_json::json!({"a": {"b": {"c": {"d": {"e": {"f": {"g": {"h": {"i": {"j": {"k": {"l": {"m": {"n": {"o": {"p": {"q": 1}}}}}}}}}}}}}}}}});
        let err = enforce_bounds(&deep, "$", 0).expect_err("depth must reject");
        assert_eq!(err.class().expect("class"), &RejectClass::DepthOverflow);

        let oversized = vec![b'a'; MAX_CONFIG_BYTES + 1];
        let err = load_json_source(&oversized).expect_err("oversize source must reject");
        assert_eq!(err.class().expect("class"), &RejectClass::SizeOverflow);

        deep = serde_json::json!([0]);
        assert!(enforce_bounds(&deep, "$", 0).is_ok());
    }

    #[test]
    fn test_layering_precedence_is_deterministic() {
        let mut inputs = sample_inputs();
        inputs.system =
            serde_json::json!({ "metrics": { "enabled": true, "flush_interval_ms": 200 } });
        inputs.state = serde_json::json!({ "metrics": { "flush_interval_ms": 300 } });
        inputs.env = serde_json::json!({ "metrics": { "enabled": false } });

        let snapshot = build_effective_snapshot(inputs).expect("layering must succeed");
        assert!(!snapshot.effective.metrics.enabled);
        assert_eq!(snapshot.effective.metrics.flush_interval_ms, 300);
    }

    #[test]
    fn test_canonical_snapshot_determinism_equivalent_inputs() {
        let inputs = sample_inputs();
        let a = build_effective_snapshot(inputs.clone()).expect("snapshot a");
        let b = build_effective_snapshot(inputs).expect("snapshot b");
        assert_eq!(a.capnp_bytes, b.capnp_bytes);
        assert_eq!(a.version, b.version);
    }

    #[test]
    fn test_changed_inputs_change_snapshot_bytes_and_version() {
        let base = build_effective_snapshot(sample_inputs()).expect("base");
        let mut changed_inputs = sample_inputs();
        changed_inputs.env = serde_json::json!({ "tracing": { "level": "debug" } });
        let changed = build_effective_snapshot(changed_inputs).expect("changed");

        assert_ne!(base.capnp_bytes, changed.capnp_bytes);
        assert_ne!(base.version, changed.version);
    }

    #[test]
    fn test_decode_roundtrip_semantic_equivalence() {
        let snapshot = build_effective_snapshot(sample_inputs()).expect("snapshot");
        let decoded = decode_effective_capnp(&snapshot.capnp_bytes).expect("decode");
        assert_eq!(decoded, snapshot.effective);
    }

    #[test]
    fn test_env_overlay_parsing_is_deterministic() {
        let mut pairs = BTreeMap::new();
        pairs.insert(
            "NEXUS_CFG_DSOFTBUS__TRANSPORT".to_string(),
            "quic".to_string(),
        );
        pairs.insert(
            "NEXUS_CFG_METRICS__ENABLED".to_string(),
            "false".to_string(),
        );
        let a = env_overrides_from_pairs(&pairs).expect("parse env a");
        let b = env_overrides_from_pairs(&pairs).expect("parse env b");
        assert_eq!(a, b);
    }

    #[test]
    fn test_load_layer_dir_merges_json_files_in_lexical_order() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("10-base.json"),
            r#"{"metrics":{"enabled":true,"flush_interval_ms":100}}"#,
        )
        .expect("write base");
        fs::write(
            temp.path().join("90-override.json"),
            r#"{"metrics":{"flush_interval_ms":300}}"#,
        )
        .expect("write override");

        let merged = load_layer_dir(temp.path()).expect("load layer dir");
        assert_eq!(merged["metrics"]["enabled"], Value::Bool(true));
        assert_eq!(
            merged["metrics"]["flush_interval_ms"],
            Value::Number(300.into())
        );
    }

    #[test]
    fn test_load_config_path_rejects_non_json_authoring_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.toml");
        fs::write(&path, "metrics = { enabled = true }").expect("write toml");
        let err = load_config_path(&path).expect_err("non-json authoring must reject");
        assert_eq!(err.class().expect("class"), &RejectClass::Serialization);
    }
}
