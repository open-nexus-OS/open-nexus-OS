//! CONTEXT: Architecture guard ensuring userspace crates remain independent of OS layers
//! OWNERS: @runtime
//! PURPOSE: Fail CI if userspace depends on OS/services beyond allowed ABI
//! NOTE: Command-line tool; no library API

use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::process::Command;

use serde_json::Value;

fn main() -> Result<(), Box<dyn Error>> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .output()?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", output.status).into());
    }

    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let packages = build_package_map(&metadata)?;
    let graph = build_dependency_graph(&metadata)?;
    let workspace_members = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or("metadata missing workspace members")?;

    // Allowlist: `nexus-abi` is a shared ABI crate used by both userland and kernel.
    let banned_names: HashSet<&str> = ["samgrd", "kernel"].into_iter().collect();
    let mut banned: HashSet<String> = packages
        .iter()
        .filter(|(_, info)| info.manifest_path.contains("/source/services/"))
        .map(|(id, _)| id.clone())
        .collect();

    for (id, info) in &packages {
        if banned_names.contains(info.name.as_str()) {
            banned.insert(id.clone());
        }
    }

    let mut violations = Vec::new();
    for member in workspace_members {
        let id = member
            .as_str()
            .ok_or("workspace member entry must be a string")?;
        let Some(info) = packages.get(id) else {
            continue;
        };
        if !info.manifest_path.contains("/userspace/") {
            continue;
        }
        if let Some(path) = find_violation(id, &graph, &banned) {
            violations.push((info.name.clone(), path));
        }
    }

    if violations.is_empty() {
        println!("arch-check: ok");
        return Ok(());
    }

    println!("arch-check: userspace crates with forbidden dependencies detected:");
    for (root, path) in violations {
        let chain = describe_path(&path, &packages);
        println!("  {root}: {chain}");
    }
    Err("userspace crates must not depend on OS layers".into())
}

#[derive(Debug)]
struct PackageInfo {
    name: String,
    manifest_path: String,
}

type PackageMap = HashMap<String, PackageInfo>;
type DependencyGraph = HashMap<String, Vec<String>>;

fn build_package_map(metadata: &Value) -> Result<PackageMap, Box<dyn Error>> {
    let mut map = HashMap::new();
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("metadata missing packages array")?;
    for package in packages {
        let Some(id) = package.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = package.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(manifest) = package.get("manifest_path").and_then(Value::as_str) else {
            continue;
        };
        map.insert(
            id.to_string(),
            PackageInfo {
                name: name.to_string(),
                manifest_path: manifest.to_string(),
            },
        );
    }
    Ok(map)
}

fn build_dependency_graph(metadata: &Value) -> Result<DependencyGraph, Box<dyn Error>> {
    let mut graph = HashMap::new();
    let nodes = metadata
        .get("resolve")
        .and_then(|resolve| resolve.get("nodes"))
        .and_then(Value::as_array)
        .ok_or("metadata missing resolve.nodes array")?;
    for node in nodes {
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            continue;
        };
        let mut deps = Vec::new();
        if let Some(dep_array) = node.get("deps").and_then(Value::as_array) {
            for dep in dep_array {
                if let Some(pkg) = dep.get("pkg").and_then(Value::as_str) {
                    deps.push(pkg.to_string());
                }
            }
        }
        graph.insert(id.to_string(), deps);
    }
    Ok(graph)
}

fn find_violation(
    start: &str,
    graph: &DependencyGraph,
    banned: &HashSet<String>,
) -> Option<Vec<String>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut parents = HashMap::new();

    visited.insert(start.to_string());
    queue.push_back(start.to_string());

    while let Some(current) = queue.pop_front() {
        if current != start && banned.contains(&current) {
            return Some(build_path(start, &current, &parents));
        }
        let Some(deps) = graph.get(&current) else {
            continue;
        };
        for dep in deps {
            if visited.insert(dep.clone()) {
                parents.insert(dep.clone(), current.clone());
                queue.push_back(dep.clone());
            }
        }
    }
    None
}

fn build_path(start: &str, end: &str, parents: &HashMap<String, String>) -> Vec<String> {
    let mut path = vec![end.to_string()];
    let mut current = end;
    while let Some(parent) = parents.get(current) {
        path.push(parent.clone());
        if parent == start {
            break;
        }
        current = parent;
    }
    path.push(start.to_string());
    path.reverse();
    path
}

fn describe_path(path: &[String], packages: &PackageMap) -> String {
    let mut names = Vec::new();
    for id in path {
        if let Some(info) = packages.get(id) {
            names.push(info.name.clone());
        } else {
            names.push(id.clone());
        }
    }
    names.join(" -> ")
}
