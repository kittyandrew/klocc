use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};

pub const SCHEMA_VERSION: i64 = 1;

pub const REQUIRED_TABLES: &[&str] = &[
    "schema_info",
    "scan",
    "scan_command",
    "scan_health",
    "store_path",
    "runtime_edge",
    "runtime_rollup",
    "ownership_rollup",
    "path_category",
    "hierarchy",
    "hierarchy_node",
    "why_depends_cache",
    "derivation",
    "derivation_output",
    "derivation_input",
    "derivation_source_input",
    "derivation_source_unit",
    "output_derivation",
    "source_unit",
    "source_loc",
    "source_dependency",
    "source_language_loc",
    "source_rollup",
    "source_treemap_node",
    "package_source",
];

#[derive(Clone, Debug)]
pub struct SourceNode {
    pub id: i64,
    pub name: String,
    pub version: Option<String>,
    pub ecosystem: String,
    pub source_kind: String,
    pub source_path: Option<String>,
    pub confidence: String,
    pub realization_status: String,
    pub own_code_loc: i64,
    pub total_code_loc: i64,
    pub unique_transitive_code_loc: i64,
    pub shared_transitive_code_loc: i64,
    pub reachable_source_count: i64,
    pub runtime_linked: bool,
    pub build_time_only: bool,
    pub derivations: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub to: i64,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct TreemapEntry {
    pub source_id: Option<i64>,
    pub label: String,
    pub group_key: String,
    pub color_key: String,
    pub source_kind: String,
    pub ecosystem: String,
    pub layer: String,
    pub own_code_loc: i64,
    pub total_code_loc: i64,
    pub unique_transitive_code_loc: i64,
    pub shared_transitive_code_loc: i64,
    pub runtime_linked: bool,
    pub build_time_only: bool,
    pub generated: bool,
    pub missing: bool,
    pub has_children: bool,
}

#[derive(Clone, Debug)]
pub struct Artifact {
    pub path: PathBuf,
    pub sources: Vec<SourceNode>,
    pub by_id: HashMap<i64, usize>,
    pub outgoing: HashMap<i64, Vec<Edge>>,
    treemap_by_view: HashMap<String, Vec<TreemapEntry>>,
    pub health: BTreeMap<String, i64>,
    pub loaded_ms: f64,
}

#[derive(Debug)]
pub struct ValidationSummary {
    pub path_count: i64,
    pub source_count: i64,
    pub unknown_sources: i64,
    pub generated_outputs: i64,
}

impl Artifact {
    pub fn load(path: &Path) -> Result<Self> {
        let started = Instant::now();
        let conn = Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        validate_for_viewer(&conn)?;

        let health = load_health(&conn)?;
        let mut derivations = load_source_derivations(&conn)?;

        let mut sources = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT su.source_id, su.name, su.version, su.ecosystem, su.source_kind, su.source_store_path, su.confidence, su.realization_status, sr.own_code_loc, sr.total_code_loc, sr.unique_transitive_code_loc, sr.shared_transitive_code_loc, sr.reachable_source_count, sr.runtime_linked, sr.build_time_only FROM source_unit su JOIN source_rollup sr USING(source_id) ORDER BY su.source_id",
        )?;
        for row in stmt.query_map([], |row| {
            Ok(SourceNode {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                ecosystem: row.get(3)?,
                source_kind: row.get(4)?,
                source_path: row.get(5)?,
                confidence: row.get(6)?,
                realization_status: row.get(7)?,
                own_code_loc: row.get(8)?,
                total_code_loc: row.get(9)?,
                unique_transitive_code_loc: row.get(10)?,
                shared_transitive_code_loc: row.get(11)?,
                reachable_source_count: row.get(12)?,
                runtime_linked: row.get(13)?,
                build_time_only: row.get(14)?,
                derivations: Vec::new(),
            })
        })? {
            let mut source = row?;
            source.derivations = derivations.remove(&source.id).unwrap_or_default();
            sources.push(source);
        }

        let by_id = sources.iter().enumerate().map(|(ix, source)| (source.id, ix)).collect();
        let outgoing = load_source_edges(&conn)?;
        let treemap_by_view = load_source_treemap(&conn, &outgoing)?;
        Ok(Self {
            path: path.to_path_buf(),
            sources,
            by_id,
            outgoing,
            treemap_by_view,
            health,
            loaded_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    pub fn source(&self, id: i64) -> Option<&SourceNode> {
        self.by_id.get(&id).and_then(|ix| self.sources.get(*ix))
    }

    pub fn treemap_entries(&self, root: Option<i64>, view_name: &str) -> Vec<TreemapEntry> {
        if let Some(root) = root {
            return self
                .outgoing
                .get(&root)
                .into_iter()
                .flatten()
                .filter_map(|edge| {
                    self.source(edge.to)
                        .map(|source| source.treemap_entry(Some(edge.kind.as_str()), &self.outgoing))
                })
                .collect();
        }
        self.treemap_by_view.get(view_name).cloned().unwrap_or_default()
    }

    pub fn has_children(&self, source_id: i64) -> bool {
        self.outgoing.get(&source_id).is_some_and(|edges| !edges.is_empty())
    }
}

impl SourceNode {
    pub fn display_name(&self) -> String {
        self.version
            .as_ref()
            .map(|version| format!("{}-{version}", self.name))
            .unwrap_or_else(|| self.name.clone())
    }

    fn treemap_entry(&self, edge_kind: Option<&str>, outgoing: &HashMap<i64, Vec<Edge>>) -> TreemapEntry {
        let label = edge_kind
            .map(|edge| format!("{} via {edge}", self.display_name()))
            .unwrap_or_else(|| self.display_name());
        TreemapEntry {
            source_id: Some(self.id),
            label,
            group_key: self.source_kind.clone(),
            color_key: self.source_kind.clone(),
            source_kind: self.source_kind.clone(),
            ecosystem: self.ecosystem.clone(),
            layer: if self.runtime_linked { "runtime" } else { "build" }.to_string(),
            own_code_loc: self.own_code_loc,
            total_code_loc: self.total_code_loc,
            unique_transitive_code_loc: self.unique_transitive_code_loc,
            shared_transitive_code_loc: self.shared_transitive_code_loc,
            runtime_linked: self.runtime_linked,
            build_time_only: self.build_time_only,
            generated: self.source_kind == "generated-derivation-output",
            missing: self.realization_status == "missing" || self.own_code_loc == 0,
            has_children: outgoing.get(&self.id).is_some_and(|edges| !edges.is_empty()),
        }
    }
}

pub fn validate_complete(conn: &Connection) -> Result<ValidationSummary> {
    validate_schema(conn)?;
    for table in REQUIRED_TABLES {
        require_table(conn, table)?;
    }

    let path_count: i64 = conn.query_row("SELECT COUNT(*) FROM store_path", [], |row| row.get(0))?;
    if path_count == 0 {
        bail!("store_path is empty");
    }
    let command_count: i64 = conn.query_row("SELECT COUNT(*) FROM scan_command", [], |row| row.get(0))?;
    if command_count == 0 {
        bail!("scan_command is empty");
    }
    let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM source_unit", [], |row| row.get(0))?;
    if source_count == 0 {
        bail!("source_unit is empty");
    }
    validate_root_path(conn)?;
    validate_foreign_keys(conn)?;
    validate_edges(conn)?;
    validate_rollup_count(conn, "runtime_rollup", path_count)?;
    validate_rollup_count(conn, "ownership_rollup", path_count)?;
    validate_rollup_count(conn, "source_rollup", source_count)?;
    validate_health(conn)?;

    Ok(ValidationSummary {
        path_count,
        source_count,
        unknown_sources: health_metric(conn, "unknown_derivation_sources")?,
        generated_outputs: health_metric(conn, "generated_derivation_outputs")?,
    })
}

pub fn validate_for_viewer(conn: &Connection) -> Result<()> {
    validate_complete(conn)?;
    validate_source_treemap(conn)?;
    Ok(())
}

fn validate_schema(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("SELECT schema_version FROM schema_info LIMIT 1", [], |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        bail!("schema version {version}; expected {SCHEMA_VERSION}");
    }
    Ok(())
}

fn require_table(conn: &Connection, table: &str) -> Result<()> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![table],
        |row| row.get(0),
    )?;
    if exists != 1 {
        bail!("missing required table: {table}");
    }
    Ok(())
}

fn validate_foreign_keys(conn: &Connection) -> Result<()> {
    let failures: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| row.get(0))?;
    if failures != 0 {
        bail!("foreign key check failed with {failures} violations");
    }
    Ok(())
}

fn validate_health(conn: &Connection) -> Result<()> {
    for metric in [
        "runtime_paths",
        "source_units",
        "source_units_with_loc",
        "unknown_derivation_sources",
        "generated_derivation_outputs",
        "derivations",
        "derivation_input_edges",
        "derivation_source_inputs",
        "derivation_source_unit_links",
        "loc_memory_cache_hits",
        "loc_persistent_cache_hits",
        "loc_cache_misses",
    ] {
        health_metric(conn, metric)?;
    }
    Ok(())
}

fn validate_source_treemap(conn: &Connection) -> Result<()> {
    let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM source_unit", [], |row| row.get(0))?;
    for view_name in ["source-kind", "ecosystem", "layer"] {
        let root_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM source_treemap_node WHERE view_name = ?1 AND parent_node_id IS NULL AND source_id IS NULL",
            params![view_name],
            |row| row.get(0),
        )?;
        if root_count != 1 {
            bail!("source_treemap_node view {view_name} has {root_count} roots; expected 1");
        }

        let leaf_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM source_treemap_node WHERE view_name = ?1 AND source_id IS NOT NULL",
            params![view_name],
            |row| row.get(0),
        )?;
        if leaf_count != source_count {
            bail!("source_treemap_node view {view_name} has {leaf_count} leaves; expected {source_count}");
        }
    }
    Ok(())
}

fn health_metric(conn: &Connection, metric: &str) -> Result<i64> {
    conn.query_row(
        "SELECT value FROM scan_health WHERE metric = ?1",
        params![metric],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| anyhow!("missing scan_health metric: {metric}"))
}

fn validate_root_path(conn: &Connection) -> Result<()> {
    let root_path: String = conn.query_row("SELECT root_store_path FROM scan LIMIT 1", [], |row| row.get(0))?;
    let root_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM store_path WHERE path = ?1",
        params![root_path],
        |row| row.get(0),
    )?;
    if root_count != 1 {
        bail!("scan root path is not present in store_path");
    }
    Ok(())
}

fn validate_edges(conn: &Connection) -> Result<()> {
    let missing_edges: i64 = conn.query_row(
        "
        SELECT COUNT(*)
        FROM runtime_edge edge
        LEFT JOIN store_path from_path ON from_path.path_id = edge.from_path_id
        LEFT JOIN store_path to_path ON to_path.path_id = edge.to_path_id
        WHERE from_path.path_id IS NULL OR to_path.path_id IS NULL
        ",
        [],
        |row| row.get(0),
    )?;
    if missing_edges != 0 {
        bail!("runtime_edge contains {missing_edges} references to missing store_path rows");
    }
    Ok(())
}

fn validate_rollup_count(conn: &Connection, table: &str, path_count: i64) -> Result<()> {
    let query = format!("SELECT COUNT(*) FROM {table}");
    let row_count: i64 = conn.query_row(&query, [], |row| row.get(0))?;
    if row_count != path_count {
        bail!("{table} row count {row_count} does not match store_path row count {path_count}");
    }
    Ok(())
}

fn load_health(conn: &Connection) -> Result<BTreeMap<String, i64>> {
    let mut health = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT metric, value FROM scan_health")?;
    for row in stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
        let (metric, value) = row?;
        health.insert(metric, value);
    }
    Ok(health)
}

fn load_source_derivations(conn: &Connection) -> Result<HashMap<i64, Vec<String>>> {
    let mut derivations = HashMap::<i64, Vec<String>>::new();
    let mut stmt = conn.prepare(
        "SELECT dsu.source_id, d.name FROM derivation_source_unit dsu JOIN derivation d USING (drv_id) ORDER BY dsu.source_id, d.name",
    )?;
    for row in stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)))? {
        let (source_id, name) = row?;
        if let Some(name) = name {
            derivations.entry(source_id).or_default().push(name);
        }
    }
    Ok(derivations)
}

fn load_source_edges(conn: &Connection) -> Result<HashMap<i64, Vec<Edge>>> {
    let mut outgoing = HashMap::<i64, Vec<Edge>>::new();
    let mut stmt = conn.prepare(
        "SELECT from_source_id, to_source_id, dependency_kind FROM source_dependency ORDER BY from_source_id, to_source_id",
    )?;
    for row in stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            Edge {
                to: row.get(1)?,
                kind: row.get(2)?,
            },
        ))
    })? {
        let (from, edge) = row?;
        outgoing.entry(from).or_default().push(edge);
    }
    Ok(outgoing)
}

fn load_source_treemap(
    conn: &Connection,
    outgoing: &HashMap<i64, Vec<Edge>>,
) -> Result<HashMap<String, Vec<TreemapEntry>>> {
    let mut by_view = HashMap::<String, Vec<TreemapEntry>>::new();
    let mut stmt = conn.prepare(
        "SELECT view_name, source_id, label, group_key, color_key, own_code_loc, total_code_loc, unique_transitive_code_loc, shared_transitive_code_loc, runtime_linked, build_time_only, source_kind, ecosystem, realization_status FROM source_treemap_node WHERE source_id IS NOT NULL ORDER BY view_name, parent_node_id, label",
    )?;
    for row in stmt.query_map([], |row| {
        let view_name: String = row.get(0)?;
        let source_id: i64 = row.get(1)?;
        let source_kind: String = row.get(11)?;
        let realization_status: String = row.get(13)?;
        let own_code_loc: i64 = row.get(5)?;
        Ok((
            view_name,
            TreemapEntry {
                source_id: Some(source_id),
                label: row.get(2)?,
                group_key: row.get(3)?,
                color_key: row.get(4)?,
                own_code_loc,
                total_code_loc: row.get(6)?,
                unique_transitive_code_loc: row.get(7)?,
                shared_transitive_code_loc: row.get(8)?,
                runtime_linked: row.get(9)?,
                build_time_only: row.get(10)?,
                generated: source_kind == "generated-derivation-output",
                missing: realization_status == "missing" || own_code_loc == 0,
                has_children: outgoing.get(&source_id).is_some_and(|edges| !edges.is_empty()),
                source_kind,
                ecosystem: row.get(12)?,
                layer: if row.get(9)? { "runtime" } else { "build" }.to_string(),
            },
        ))
    })? {
        let (view_name, entry) = row?;
        by_view.entry(view_name).or_default().push(entry);
    }
    Ok(by_view)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn source_node_treemap_entries_preserve_read_model_invariants(
            id in 1_i64..10_000,
            name in "[a-z][a-z0-9_-]{0,16}",
            version in prop::option::of("[0-9][0-9a-z._-]{0,12}"),
            source_kind in prop_oneof![Just("generated-derivation-output".to_string()), "[a-z][a-z-]{0,24}"],
            ecosystem in "[a-z][a-z-]{0,16}",
            own_code_loc in 0_i64..100_000,
            total_code_loc in 0_i64..1_000_000,
            unique_transitive_code_loc in 0_i64..1_000_000,
            shared_transitive_code_loc in 0_i64..1_000_000,
            runtime_linked in any::<bool>(),
            realization_missing in any::<bool>(),
            child_count in 0_usize..5,
            edge_kind in prop::option::of("[a-z][a-z-]{0,16}"),
        ) {
            let node = SourceNode {
                id,
                name: name.clone(),
                version: version.clone(),
                ecosystem: ecosystem.clone(),
                source_kind: source_kind.clone(),
                source_path: Some(format!("/nix/store/{name}")),
                confidence: "test".to_string(),
                realization_status: if realization_missing { "missing" } else { "realized" }.to_string(),
                own_code_loc,
                total_code_loc,
                unique_transitive_code_loc,
                shared_transitive_code_loc,
                reachable_source_count: child_count as i64,
                runtime_linked,
                build_time_only: !runtime_linked,
                derivations: Vec::new(),
            };
            let mut outgoing = HashMap::new();
            if child_count > 0 {
                outgoing.insert(
                    id,
                    (0..child_count)
                        .map(|offset| Edge {
                            to: id + offset as i64 + 1,
                            kind: "test-edge".to_string(),
                        })
                        .collect::<Vec<_>>(),
                );
            }

            let entry = node.treemap_entry(edge_kind.as_deref(), &outgoing);

            prop_assert_eq!(entry.source_id, Some(id));
            prop_assert_eq!(entry.group_key.as_str(), source_kind.as_str());
            prop_assert_eq!(entry.color_key.as_str(), source_kind.as_str());
            prop_assert_eq!(entry.source_kind.as_str(), source_kind.as_str());
            prop_assert_eq!(entry.ecosystem.as_str(), ecosystem.as_str());
            prop_assert_eq!(entry.layer, if runtime_linked { "runtime" } else { "build" });
            prop_assert_eq!(entry.own_code_loc, own_code_loc);
            prop_assert_eq!(entry.total_code_loc, total_code_loc);
            prop_assert_eq!(entry.unique_transitive_code_loc, unique_transitive_code_loc);
            prop_assert_eq!(entry.shared_transitive_code_loc, shared_transitive_code_loc);
            prop_assert_eq!(entry.runtime_linked, runtime_linked);
            prop_assert_eq!(entry.build_time_only, !runtime_linked);
            prop_assert_eq!(entry.generated, source_kind == "generated-derivation-output");
            prop_assert_eq!(entry.missing, realization_missing || own_code_loc == 0);
            prop_assert_eq!(entry.has_children, child_count > 0);

            let display_name = version
                .as_ref()
                .map(|version| format!("{name}-{version}"))
                .unwrap_or(name);
            if let Some(edge_kind) = edge_kind {
                prop_assert_eq!(entry.label, format!("{display_name} via {edge_kind}"));
            } else {
                prop_assert_eq!(entry.label, display_name);
            }
        }
    }
}
