use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::artifact::{self, SCHEMA_VERSION};

pub fn run(artifact: &Path) -> Result<()> {
    let conn = Connection::open(artifact).with_context(|| format!("failed to open {}", artifact.display()))?;
    let summary = artifact::validate_complete(&conn)?;

    println!("ok: schema version {SCHEMA_VERSION}");
    println!("ok: {} store paths", summary.path_count);
    println!("ok: {} source units", summary.source_count);
    println!(
        "ok: {} unknown derivation sources, {} generated derivation outputs",
        summary.unknown_sources, summary.generated_outputs
    );
    println!("ok: graph integrity checks passed");
    Ok(())
}
