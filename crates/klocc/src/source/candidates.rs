#[derive(Clone, Debug)]
pub(super) struct SourceCandidate {
    pub(super) path: String,
    pub(super) source_kind: String,
    pub(super) confidence: String,
    pub(super) relationship: String,
}

pub(super) fn source_env_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "src"
        || lower == "srcs"
        || lower.contains("source")
        || lower.contains("patch")
        || lower.contains("vendor")
        || lower.contains("cargodeps")
        || lower.contains("cargovendordir")
}

pub(super) fn source_kind_for_env_key(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    if lower.contains("patch") {
        "patch-source".to_string()
    } else if lower.contains("vendor") || lower.contains("cargodeps") || lower.contains("cargovendordir") {
        "cargo-vendor-dir".to_string()
    } else {
        format!("derivation-env-{key}")
    }
}

pub(super) fn relationship_for_env_key(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    if lower.contains("patch") {
        "patch-source".to_string()
    } else if lower.contains("vendor") || lower.contains("cargodeps") || lower.contains("cargovendordir") {
        "vendored-source".to_string()
    } else {
        "package-source".to_string()
    }
}

pub(super) fn source_like_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    source_like_name(name)
}

pub(super) fn source_candidate_path_from_reference(path: &str) -> Option<String> {
    let root = store_path_root(path)?;
    if source_like_path(&root) {
        return Some(root);
    }
    source_like_path(path).then(|| path.to_string())
}

fn store_path_root(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/nix/store/")?;
    let name = rest.split('/').next()?;
    (!name.is_empty()).then(|| format!("/nix/store/{name}"))
}

pub(super) fn source_like_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("source")
        || lower.contains("src")
        || lower.contains("vendor")
        || lower.contains("cargo-package")
        || lower.contains("tarball")
        || lower.ends_with(".crate")
        || lower.ends_with(".tar")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tgz")
        || lower.ends_with(".patch")
        || lower.ends_with(".diff")
        || lower.ends_with(".c")
        || lower.ends_with(".h")
        || lower.ends_with(".cc")
        || lower.ends_with(".cpp")
        || lower.ends_with(".rs")
        || lower.ends_with(".sh")
        || lower.ends_with(".pl")
        || lower.ends_with(".py")
        || lower.ends_with(".m4")
        || lower.ends_with(".ac")
        || lower.ends_with(".mk")
        || lower.ends_with(".in")
        || lower.ends_with(".zip")
}

pub(super) fn store_path_from_json_path(path: &str) -> String {
    if path.starts_with("/nix/store/") {
        path.to_string()
    } else {
        format!("/nix/store/{path}")
    }
}

pub(super) fn extract_store_paths(text: &str) -> Vec<String> {
    text.split(|ch: char| ch.is_whitespace() || ch == ':' || ch == ';' || ch == ',')
        .filter_map(|token| {
            let start = token.find("/nix/store/")?;
            let path = &token[start..];
            let end = path.find(['"', '\'', ')', ']', '}']).unwrap_or(path.len());
            Some(path[..end].to_string())
        })
        .collect()
}
