// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pipeline library build (TASK-0215 5c-2 P4c, ADR-0072 §5–7, RFC-0101 §7b): a manifest
//! declares each variant once — its id, name, source, entry point, overrides, blend and
//! instruction-group budget. Each is compiled, held to its budget (the host build fails above
//! it), given its setup programs, and the archive is built for the manifest's code target.
//!
//! ```toml
//! target = "board"
//! [[pipeline]]
//! id = 1
//! name = "layer-opaque"
//! source = "../shaders/layer.wgsl"   # relative to the manifest
//! entry = "main"                      # the default
//! overrides = { tinted = false }      # bools and numbers, keyed as the pipeline's constants
//! blend = "replace"                   # or "premultiplied"
//! budget = 3
//! ```

use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::library::{self, EntrySpec};
use nexus_gfx::shader::rogue::codegen::Layout;
use nexus_gfx::shader::rogue::encode_program;
use nexus_gfx::shader::rogue::setup::{fragment, Varyings};

use super::{compile_fragment, Blend};

/// A variant as the manifest declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipeline {
    pub id: u32,
    pub name: String,
    pub source: String,
    pub entry: String,
    pub overrides: Vec<(String, f64)>,
    pub blend: Blend,
    pub budget: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub target: u64,
    pub pipelines: Vec<Pipeline>,
}

/// What a built entry is, for the tool's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    pub id: u32,
    pub name: String,
    pub groups: usize,
    pub budget: u16,
    pub temps: u8,
    pub pass_type: u8,
    pub bytes: usize,
}

fn int(t: &toml::Table, key: &str, what: &str, max: u64) -> Result<u64, String> {
    match t.get(key) {
        Some(toml::Value::Integer(v)) if (1..=max as i64).contains(v) => Ok(*v as u64),
        _ => Err(format!("{what}: `{key}` must be an integer from 1 to {max}")),
    }
}

fn string(t: &toml::Table, key: &str, what: &str) -> Result<String, String> {
    match t.get(key) {
        Some(toml::Value::String(s)) => Ok(s.clone()),
        _ => Err(format!("{what}: `{key}` must be a string")),
    }
}

/// The manifest's variants; unknown keys, targets and blends are refused by name.
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("manifest: {e}"))?;
    if let Some(k) = t.keys().find(|k| !matches!(k.as_str(), "target" | "pipeline")) {
        return Err(format!("manifest: unknown key `{k}`"));
    }
    let target = match string(&t, "target", "manifest")?.as_str() {
        "board" => CoreId::BOARD.0,
        other => return Err(format!("manifest: unknown code target `{other}`")),
    };
    let Some(toml::Value::Array(list)) = t.get("pipeline") else {
        return Err("manifest: no `[[pipeline]]`".into());
    };
    let mut pipelines = Vec::with_capacity(list.len());
    for v in list {
        let toml::Value::Table(p) = v else {
            return Err("manifest: a pipeline is not a table".into());
        };
        let known = ["id", "name", "source", "entry", "overrides", "blend", "budget"];
        if let Some(k) = p.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(format!("pipeline: unknown key `{k}`"));
        }
        let name = string(p, "name", "pipeline")?;
        let what = format!("pipeline {name}");
        let blend = match string(p, "blend", &what)?.as_str() {
            "replace" => Blend::Replace,
            "premultiplied" => Blend::OverPremultiplied,
            other => return Err(format!("{what}: unknown blend `{other}`")),
        };
        let mut overrides = Vec::new();
        if let Some(o) = p.get("overrides") {
            let toml::Value::Table(o) = o else {
                return Err(format!("{what}: `overrides` must be a table"));
            };
            for (k, v) in o {
                let x = match v {
                    toml::Value::Boolean(b) => f64::from(u8::from(*b)),
                    toml::Value::Integer(i) => *i as f64,
                    toml::Value::Float(f) => *f,
                    _ => return Err(format!("{what}: override `{k}` must be a bool or a number")),
                };
                overrides.push((k.clone(), x));
            }
        }
        pipelines.push(Pipeline {
            id: int(p, "id", &what, u64::from(u32::MAX))? as u32,
            entry: match p.get("entry") {
                None => "main".into(),
                Some(_) => string(p, "entry", &what)?,
            },
            source: string(p, "source", &what)?,
            overrides,
            blend,
            budget: int(p, "budget", &what, u64::from(u16::MAX))? as u16,
            name,
        });
    }
    Ok(Manifest { target, pipelines })
}

/// The archive of the manifest's variants (each source read through `source`), with what each
/// entry is.
pub fn build(
    manifest: &Manifest,
    source: impl Fn(&str) -> Result<String, String>,
) -> Result<(Vec<u8>, Vec<Built>), String> {
    struct Part {
        interface: nexus_gfx::shader::ir::Interface,
        program: Vec<u8>,
        setup: Vec<Vec<u8>>,
        state: Vec<u8>,
        features: u32,
    }
    let mut parts = Vec::with_capacity(manifest.pipelines.len());
    let mut built = Vec::with_capacity(manifest.pipelines.len());
    for p in &manifest.pipelines {
        let src = source(&p.source)?;
        let overrides: Vec<(&str, f64)> =
            p.overrides.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let c = compile_fragment(&src, &p.entry, &overrides, p.blend, true)
            .map_err(|e| format!("{}: {e:?}", p.name))?;
        let groups = c.program.groups.len();
        if groups > usize::from(p.budget) {
            return Err(format!("{}: {groups} groups above its budget of {}", p.name, p.budget));
        }
        let program =
            encode_program(&c.program.groups).map_err(|e| format!("{}: {e:?}", p.name))?;
        let setup = fragment(&c.program, &c.interface, Layout::BUNDLE, Varyings::BUNDLE)
            .map_err(|e| format!("{}: setup {e:?}", p.name))?;
        let (sections, state) = setup.sections().map_err(|e| format!("{}: setup {e:?}", p.name))?;
        built.push(Built {
            id: p.id,
            name: p.name.clone(),
            groups,
            budget: p.budget,
            temps: c.program.temps,
            pass_type: c.program.pass_type,
            bytes: program.len(),
        });
        parts.push(Part {
            interface: c.interface,
            program,
            setup: sections,
            state,
            features: c.program.features(),
        });
    }
    let refs: Vec<Vec<&[u8]>> =
        parts.iter().map(|q| q.setup.iter().map(Vec::as_slice).collect()).collect();
    let specs: Vec<EntrySpec<'_>> = manifest
        .pipelines
        .iter()
        .zip(&parts)
        .zip(&refs)
        .map(|((p, q), setup)| EntrySpec {
            id: p.id,
            name: &p.name,
            features: q.features,
            budget: p.budget,
            interface: &q.interface,
            program: &q.program,
            setup,
            state: &q.state,
        })
        .collect();
    let bytes = library::build(manifest.target, &specs).map_err(|e| format!("library: {e:?}"))?;
    Ok((bytes, built))
}
