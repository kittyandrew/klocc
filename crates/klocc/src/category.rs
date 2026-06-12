use crate::model::Category;

pub fn classify(name: &str) -> Category {
    let lower = name.to_ascii_lowercase();
    let category = if lower.contains("firmware") {
        "firmware"
    } else if lower.contains("linux") || lower.contains("kernel") {
        "kernel"
    } else if lower.contains("gcc")
        || lower.contains("clang")
        || lower.contains("rustc")
        || lower.contains("cargo")
        || lower.contains("compiler")
    {
        "toolchain"
    } else if lower.ends_with("-src")
        || lower.ends_with("-source")
        || lower.contains("source")
        || lower.contains("tarball")
        || lower.contains("vendor")
    {
        "source"
    } else if lower.contains("lib") || lower.contains("openssl") || lower.contains("glibc") {
        "library"
    } else if lower.contains("doc") || lower.contains("man") || lower.contains("share") {
        "data"
    } else {
        "unknown"
    };

    Category {
        name: category.to_string(),
        confidence: if category == "unknown" { 0.2 } else { 0.6 },
        reason: "phase-1 name heuristic".to_string(),
    }
}
