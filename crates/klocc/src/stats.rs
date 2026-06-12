use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::format;

pub fn run(artifact: &Path) -> Result<()> {
    let conn = Connection::open(artifact).with_context(|| format!("failed to open {}", artifact.display()))?;
    let (root_input, root_store_path, nix_version): (String, String, Option<String>) = conn.query_row(
        "SELECT root_input, root_store_path, nix_version FROM scan LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let path_count: i64 = conn.query_row("SELECT COUNT(*) FROM store_path", [], |row| row.get(0))?;
    let edge_count: i64 = conn.query_row("SELECT COUNT(*) FROM runtime_edge", [], |row| row.get(0))?;
    let total_nar_size: i64 = conn.query_row("SELECT COALESCE(SUM(nar_size), 0) FROM store_path", [], |row| {
        row.get(0)
    })?;
    let root_closure_size: Option<i64> = conn
        .query_row(
            "SELECT closure_size FROM store_path WHERE path = ?1",
            params![root_store_path],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let unknown_deriver_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM store_path WHERE deriver_status = 'unknown-deriver'",
        [],
        |row| row.get(0),
    )?;
    let (unique_bytes, shared_bytes): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(unique_bytes), 0), COALESCE(SUM(shared_bytes), 0) FROM ownership_rollup",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM source_unit", [], |row| row.get(0))?;
    let loc_total: i64 = conn.query_row("SELECT COALESCE(SUM(loc_total), 0) FROM source_loc", [], |row| {
        row.get(0)
    })?;
    let loc_code: i64 = conn.query_row("SELECT COALESCE(SUM(loc_code), 0) FROM source_loc", [], |row| {
        row.get(0)
    })?;

    println!("scan: {}", artifact.display());
    println!("root input: {root_input}");
    println!("root path: {root_store_path}");
    println!("nix: {}", nix_version.as_deref().unwrap_or("unknown"));
    println!();
    println!("runtime closure:");
    println!("  paths: {path_count}");
    println!("  edges: {edge_count}");
    println!("  nar size: {}", format::bytes(total_nar_size));
    println!(
        "  root closure size: {}",
        root_closure_size
            .map(format::bytes)
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!("  unknown derivers: {unknown_deriver_count}");
    println!("  unique bytes: {}", format::bytes(unique_bytes));
    println!("  shared bytes: {}", format::bytes(shared_bytes));
    println!("  source units: {source_count}");
    println!("  source LOC: {loc_total} total, {loc_code} code");
    print_health(&conn)?;
    print_source_breakdown(&conn)?;
    print_top(
        &conn,
        "top nar size",
        "SELECT path, COALESCE(nar_size, 0) FROM store_path ORDER BY COALESCE(nar_size, 0) DESC, path LIMIT 20",
    )?;
    print_top(
        &conn,
        "top closure size",
        "SELECT path, COALESCE(closure_size, 0) FROM store_path ORDER BY COALESCE(closure_size, 0) DESC, path LIMIT 20",
    )?;
    print_top(
        &conn,
        "top reverse ref count",
        "SELECT store_path.path, runtime_rollup.reverse_ref_count FROM store_path JOIN runtime_rollup USING (path_id) ORDER BY runtime_rollup.reverse_ref_count DESC, store_path.path LIMIT 20",
    )?;
    print_top(
        &conn,
        "top owner count",
        "SELECT store_path.path, ownership_rollup.top_owner_count FROM store_path JOIN ownership_rollup USING (path_id) ORDER BY ownership_rollup.top_owner_count DESC, store_path.path LIMIT 20",
    )?;

    Ok(())
}

fn print_source_breakdown(conn: &Connection) -> Result<()> {
    println!();
    println!("source health:");
    let mut stmt = conn.prepare(
        "SELECT source_kind, realization_status, COUNT(*) FROM source_unit GROUP BY source_kind, realization_status ORDER BY COUNT(*) DESC, source_kind LIMIT 20",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (source_kind, realization_status, count) = row?;
        println!("  {count:>5}  {source_kind} / {realization_status}");
    }
    Ok(())
}

fn print_health(conn: &Connection) -> Result<()> {
    println!();
    println!("scan health:");
    let mut stmt = conn.prepare("SELECT metric, value FROM scan_health ORDER BY metric")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    for row in rows {
        let (metric, value) = row?;
        println!("  {metric}: {value}");
    }
    Ok(())
}

fn print_top(conn: &Connection, title: &str, query: &str) -> Result<()> {
    println!();
    println!("{title}:");
    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    for (index, row) in rows.enumerate() {
        let (path, value) = row?;
        let display_value = if title.contains("size") {
            format::bytes(value)
        } else {
            value.to_string()
        };
        println!("  {:>2}. {:>11}  {path}", index + 1, display_value);
    }
    Ok(())
}
