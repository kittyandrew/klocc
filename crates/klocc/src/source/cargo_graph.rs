use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::model::SourceUnit;

use super::naming::{StartsWithStore, cargo_key, split_crate_dir};
use super::{LocMeasureContext, SourceMeasurement, builder::SourceBuilder, measure_source_path};

#[derive(Deserialize)]
struct CargoLock {
    package: Vec<CargoLockPackage>,
}

#[derive(Deserialize)]
struct CargoLockPackage {
    name: String,
    version: String,
    source: Option<String>,
    dependencies: Option<Vec<String>>,
}

pub(super) fn add_cargo_graph(
    builder: &mut SourceBuilder,
    root_source_path: &Path,
    vendor_paths: &[PathBuf],
    root_name: &str,
    root_source_index: usize,
    loc_context: &mut LocMeasureContext<'_>,
) -> Result<()> {
    let lock_path = root_source_path.join("Cargo.lock");
    if !lock_path.exists() {
        return Ok(());
    }

    let lock: CargoLock = toml::from_str(
        &fs::read_to_string(&lock_path).with_context(|| format!("failed to read {}", lock_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", lock_path.display()))?;
    let vendor_index = index_vendor_paths(vendor_paths);
    let mut package_to_source = HashMap::new();
    let mut by_name = HashMap::<String, Vec<String>>::new();
    let mut dependency_kinds_by_package = HashMap::<String, HashMap<String, Vec<String>>>::new();

    for package in &lock.package {
        let package_key = cargo_key(&package.name, &package.version);
        by_name
            .entry(package.name.clone())
            .or_default()
            .push(package_key.clone());
        if package.source.is_none() && package.name == root_name {
            package_to_source.insert(package_key.clone(), root_source_index);
            let manifest_path = root_source_path.join("Cargo.toml");
            dependency_kinds_by_package.insert(
                package_key.clone(),
                dependency_kinds_from_manifest(&manifest_path).unwrap_or_default(),
            );
            continue;
        }
        if package.source.is_none() {
            continue;
        }

        let path = vendor_index.get(&package_key).cloned();
        let source_store_path = path
            .as_ref()
            .filter(|path| path.starts_with("/nix/store/"))
            .map(|path| path.display().to_string());
        let origin_url = path
            .as_ref()
            .filter(|path| !path.starts_with("/nix/store/"))
            .map(|path| format!("file://{}", path.display()));
        let measurement = match path.as_ref() {
            Some(path) => measure_source_path(
                path,
                loc_context.runner,
                loc_context.loc_cache,
                loc_context.persistent_loc_cache.as_deref_mut(),
                loc_context.loc_stats,
            )?,
            None => SourceMeasurement {
                realization_status: "metadata-only".into(),
                loc: None,
            },
        };
        let source_index = builder.add_unit(SourceUnit {
            name: package.name.clone(),
            version: Some(package.version.clone()),
            ecosystem: "cargo".to_string(),
            source_store_path,
            origin_url,
            origin_rev: None,
            source_kind: if path.is_some() {
                "cargo-vendored-crate"
            } else {
                "cargo-lock-crate"
            }
            .into(),
            confidence: if path.is_some() { "high" } else { "medium" }.into(),
            realization_status: measurement.realization_status,
            links: Vec::new(),
            loc: measurement.loc,
        });
        if let Some(path) = &path {
            dependency_kinds_by_package.insert(
                package_key.clone(),
                dependency_kinds_from_manifest(&path.join("Cargo.toml")).unwrap_or_default(),
            );
        }
        package_to_source.insert(package_key, source_index);
    }

    for package in &lock.package {
        let package_key = cargo_key(&package.name, &package.version);
        let Some(from_index) = package_to_source.get(&package_key).copied() else {
            continue;
        };
        for dep_spec in package.dependencies.as_deref().unwrap_or(&[]) {
            if let Some(to_key) = resolve_cargo_dep(dep_spec, &by_name)
                && let Some(to_index) = package_to_source.get(&to_key).copied()
            {
                let dep_name = dep_name_from_spec(dep_spec);
                let kinds = dependency_kinds_by_package
                    .get(&package_key)
                    .and_then(|kinds| kinds.get(&dep_name))
                    .cloned()
                    .unwrap_or_else(|| vec!["cargo:lock-only".to_string()]);
                for kind in kinds {
                    builder.add_dependency(from_index, to_index, &kind, Some(dep_spec.to_string()));
                }
            }
        }
    }

    Ok(())
}

fn dependency_kinds_from_manifest(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let value: toml::Value =
        toml::from_str(&fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?)
            .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut kinds = HashMap::<String, Vec<String>>::new();
    collect_manifest_deps(&value, "dependencies", "cargo:normal", &mut kinds);
    collect_manifest_deps(&value, "build-dependencies", "cargo:build", &mut kinds);
    collect_manifest_deps(&value, "dev-dependencies", "cargo:dev", &mut kinds);
    if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            collect_manifest_deps(target, "dependencies", "cargo:target-normal", &mut kinds);
            collect_manifest_deps(target, "build-dependencies", "cargo:target-build", &mut kinds);
            collect_manifest_deps(target, "dev-dependencies", "cargo:target-dev", &mut kinds);
        }
    }
    Ok(kinds)
}

fn collect_manifest_deps(value: &toml::Value, section: &str, kind: &str, kinds: &mut HashMap<String, Vec<String>>) {
    let Some(table) = value.get(section).and_then(toml::Value::as_table) else {
        return;
    };
    for (name, dependency) in table {
        let resolved_name = dependency.get("package").and_then(toml::Value::as_str).unwrap_or(name);
        let entries = kinds.entry(resolved_name.to_string()).or_default();
        if !entries.iter().any(|entry| entry == kind) {
            entries.push(kind.to_string());
        }
    }
}

fn index_vendor_paths(vendor_paths: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut indexed = HashMap::new();
    for vendor_path in vendor_paths {
        index_vendor_dir(vendor_path, 0, &mut indexed);
    }
    indexed
}

fn index_vendor_dir(path: &Path, depth: usize, indexed: &mut HashMap<String, PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Some(file_name) = entry_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some((name, version)) = split_crate_dir(file_name) {
            let real_path = entry_path.canonicalize().unwrap_or(entry_path.clone());
            indexed.insert(cargo_key(&name, &version), real_path);
            continue;
        }

        let Ok(file_type) = fs::symlink_metadata(&entry_path).map(|metadata| metadata.file_type()) else {
            continue;
        };
        if file_type.is_dir() || file_type.is_symlink() {
            let next_path = entry_path.canonicalize().unwrap_or(entry_path);
            if next_path.is_dir() {
                index_vendor_dir(&next_path, depth + 1, indexed);
            }
        }
    }
}

fn resolve_cargo_dep(dep_spec: &str, by_name: &HashMap<String, Vec<String>>) -> Option<String> {
    let mut parts = dep_spec.split_whitespace();
    let name = parts.next()?;
    let version = parts.next();
    if let Some(version) = version
        && version.chars().next().is_some_and(|ch| ch.is_ascii_digit())
    {
        return Some(cargo_key(name, version));
    }
    let candidates = by_name.get(name)?;
    (candidates.len() == 1).then(|| candidates[0].clone())
}

fn dep_name_from_spec(dep_spec: &str) -> String {
    dep_spec.split_whitespace().next().unwrap_or(dep_spec).to_string()
}
