use std::path::{Path, PathBuf};

pub(super) fn unit_key(ecosystem: &str, name: &str, version: Option<&str>, path: Option<&str>) -> String {
    if let Some(path) = path {
        return format!("{ecosystem}:{path}");
    }
    format!("{ecosystem}:{name}:{}:{}", version.unwrap_or(""), path.unwrap_or(""))
}

pub(super) fn cargo_key(name: &str, version: &str) -> String {
    format!("{name} {version}")
}

pub(super) fn split_crate_dir(name: &str) -> Option<(String, String)> {
    let name = strip_store_hash_prefix(name);
    let (name, version) = name.rsplit_once('-')?;
    let name = name.strip_prefix("cargo-package-").unwrap_or(name);
    version
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
        .then(|| (name.to_string(), version.to_string()))
}

pub(super) fn package_name_from_store_name(name: &str) -> String {
    let name = strip_store_hash_prefix(name);
    match name.rsplit_once('-') {
        Some((prefix, suffix)) if suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit()) => prefix.to_string(),
        _ => name.to_string(),
    }
}

pub(super) fn version_from_store_name(name: &str) -> Option<String> {
    let name = strip_store_hash_prefix(name);
    match name.rsplit_once('-') {
        Some((_, suffix)) if suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit()) => Some(suffix.to_string()),
        _ => None,
    }
}

fn strip_store_hash_prefix(name: &str) -> &str {
    let Some((prefix, rest)) = name.split_once('-') else {
        return name;
    };
    if prefix.len() >= 20 && prefix.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        rest
    } else {
        name
    }
}

pub(super) trait StartsWithStore {
    fn starts_with(&self, prefix: &str) -> bool;
}

impl StartsWithStore for PathBuf {
    fn starts_with(&self, prefix: &str) -> bool {
        self.to_string_lossy().starts_with(prefix)
    }
}

impl StartsWithStore for Path {
    fn starts_with(&self, prefix: &str) -> bool {
        self.to_string_lossy().starts_with(prefix)
    }
}
