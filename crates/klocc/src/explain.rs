use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::nix::NixRunner;
use crate::time;

pub fn run(artifact: &Path, target: &str) -> Result<()> {
    let conn = Connection::open(artifact).with_context(|| format!("failed to open {}", artifact.display()))?;
    let root_path: String = conn.query_row("SELECT root_store_path FROM scan LIMIT 1", [], |row| row.get(0))?;
    let root_path_id = path_id(&conn, &root_path)?.ok_or_else(|| anyhow!("root path is not present in artifact"))?;
    let target_path_id =
        path_id(&conn, target)?.ok_or_else(|| anyhow!("target store path is not present in artifact: {target}"))?;

    if let Some(stdout) = cached_explanation(&conn, root_path_id, target_path_id)? {
        print!("{stdout}");
        return Ok(());
    }

    let mut runner = NixRunner::new();
    let output = runner.why_depends_precise(&root_path, target)?;
    let command = runner
        .last_command()
        .ok_or_else(|| anyhow!("internal error: why-depends command was not recorded"))?
        .clone();
    record_command(&conn, &command)?;

    if output.exit_code != 0 {
        bail!(
            "command failed: {}\nexit code: {}\nstderr: {}",
            command.command,
            output.exit_code,
            output.stderr
        );
    }

    conn.execute(
        "INSERT INTO why_depends_cache (root_path_id, target_path_id, mode, created_at, stdout, parsed_json) VALUES (?1, ?2, 'precise', ?3, ?4, NULL)",
        params![root_path_id, target_path_id, time::unix_timestamp()?, output.stdout],
    )?;
    print!("{}", output.stdout);
    Ok(())
}

fn path_id(conn: &Connection, path: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT path_id FROM store_path WHERE path = ?1", params![path], |row| {
            row.get(0)
        })
        .optional()?)
}

fn cached_explanation(conn: &Connection, root_path_id: i64, target_path_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT stdout FROM why_depends_cache WHERE root_path_id = ?1 AND target_path_id = ?2 AND mode = 'precise'",
            params![root_path_id, target_path_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn record_command(conn: &Connection, command: &crate::model::CommandRun) -> Result<()> {
    let scan_id: String = conn.query_row("SELECT scan_id FROM scan LIMIT 1", [], |row| row.get(0))?;
    let next_seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM scan_command WHERE scan_id = ?1",
        params![scan_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO scan_command (scan_id, seq, command, exit_code, duration_ms, stderr_excerpt) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            scan_id,
            next_seq,
            command.command,
            command.exit_code,
            command.duration_ms,
            command.stderr_excerpt
        ],
    )?;
    Ok(())
}
