use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::json;

use crate::db::{CREATED_BY, SCHEMA_VERSION, schema};
use crate::hierarchy;
use crate::model::ScanData;
use crate::time;

pub fn write(out: &Path, scan: &ScanData) -> Result<()> {
    let mut conn = Connection::open(out).with_context(|| format!("failed to create {}", out.display()))?;
    schema::create(&conn)?;
    let tx = conn.transaction()?;
    let created_at = time::unix_timestamp()?;
    let scan_id = format!("scan-{created_at}-{}", std::process::id());

    tx.execute(
        "INSERT INTO schema_info (schema_version, created_by, created_at) VALUES (?1, ?2, ?3)",
        params![SCHEMA_VERSION, CREATED_BY, created_at],
    )?;
    tx.execute(
        "INSERT INTO scan (scan_id, root_input, root_store_path, nix_version, policy_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            scan_id,
            scan.root_input,
            scan.root_store_path,
            scan.nix_version,
            json!({
                "phase": 1,
                "loc": "tokei source-unit counting for local package source, per-crate Cargo vendor sources, and derivation-discovered sources when available",
                "installable_resolution": "non-store roots are realized with nix build --no-link --print-out-paths before path-info",
                "ownership": "root-direct-reference top-owner rollup plus hierarchy-local duplicate-aware treemap metrics",
            })
            .to_string()
        ],
    )?;

    for (seq, command) in scan.commands.iter().enumerate() {
        tx.execute(
            "INSERT INTO scan_command (scan_id, seq, command, exit_code, duration_ms, stderr_excerpt) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![scan_id, seq as i64 + 1, command.command, command.exit_code, command.duration_ms, command.stderr_excerpt],
        )?;
    }
    for metric in &scan.health_metrics {
        tx.execute(
            "INSERT INTO scan_health (scan_id, metric, value) VALUES (?1, ?2, ?3)",
            params![scan_id, metric.name, metric.value],
        )?;
    }

    let path_ids = insert_paths(&tx, scan)?;
    insert_edges(&tx, scan, &path_ids)?;
    insert_rollups(&tx, scan, &path_ids)?;
    insert_sources(&tx, scan, &path_ids)?;
    insert_derivations(&tx, scan, &path_ids)?;
    hierarchy::insert(&tx, scan, &path_ids)?;
    tx.commit()?;
    Ok(())
}

fn insert_derivations(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    let mut drv_ids = std::collections::BTreeMap::new();
    for derivation in &scan.derivations {
        tx.execute(
            "INSERT INTO derivation (drv_path, name, system, builder, is_fixed_output, raw_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                derivation.drv_path,
                derivation.name.as_deref(),
                derivation.system.as_deref(),
                derivation.builder.as_deref(),
                derivation.is_fixed_output,
                derivation.raw_json,
            ],
        )?;
        drv_ids.insert(derivation.drv_path.clone(), tx.last_insert_rowid());
    }

    for (index, node) in scan.nodes.iter().enumerate() {
        let Some(deriver_path) = &node.deriver_path else {
            continue;
        };
        let drv_id = ensure_drv_id(tx, &mut drv_ids, deriver_path, Some(&node.name))?;
        tx.execute(
            "INSERT INTO output_derivation (path_id, drv_id, output_name) VALUES (?1, ?2, ?3)",
            params![path_ids[index], drv_id, "out"],
        )?;
    }

    for derivation in &scan.derivations {
        let drv_id = ensure_drv_id(tx, &mut drv_ids, &derivation.drv_path, derivation.name.as_deref())?;
        for output in &derivation.outputs {
            tx.execute(
                "INSERT OR IGNORE INTO derivation_output (drv_id, output_name, output_path) VALUES (?1, ?2, ?3)",
                params![drv_id, output.output_name, output.output_path.as_deref()],
            )?;
        }
        for input in &derivation.input_derivations {
            let input_drv_id = ensure_drv_id(tx, &mut drv_ids, &input.input_drv_path, None)?;
            tx.execute(
                "INSERT OR IGNORE INTO derivation_input (drv_id, input_drv_id, output_names_json) VALUES (?1, ?2, ?3)",
                params![drv_id, input_drv_id, serde_json::to_string(&input.output_names)?],
            )?;
        }
        for source_path in &derivation.input_sources {
            tx.execute(
                "INSERT OR IGNORE INTO derivation_source_input (drv_id, source_path) VALUES (?1, ?2)",
                params![drv_id, source_path],
            )?;
        }
        for link in &derivation.source_links {
            tx.execute(
                "INSERT OR IGNORE INTO derivation_source_unit (drv_id, source_id, relationship) VALUES (?1, ?2, ?3)",
                params![drv_id, link.source_index as i64 + 1, link.relationship.as_str()],
            )?;
        }
    }
    Ok(())
}

fn ensure_drv_id(
    tx: &rusqlite::Transaction<'_>,
    drv_ids: &mut std::collections::BTreeMap<String, i64>,
    drv_path: &str,
    name: Option<&str>,
) -> Result<i64> {
    if let Some(drv_id) = drv_ids.get(drv_path) {
        return Ok(*drv_id);
    }
    tx.execute(
        "INSERT INTO derivation (drv_path, name, system, builder, is_fixed_output, raw_json) VALUES (?1, ?2, NULL, NULL, NULL, NULL)",
        params![drv_path, name],
    )?;
    let drv_id = tx.last_insert_rowid();
    drv_ids.insert(drv_path.to_string(), drv_id);
    Ok(drv_id)
}

fn insert_sources(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    for source in &scan.sources {
        tx.execute(
            "INSERT INTO source_unit (name, version, ecosystem, source_store_path, origin_url, origin_rev, source_kind, confidence, realization_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                source.name,
                source.version.as_deref(),
                source.ecosystem,
                source.source_store_path.as_deref(),
                source.origin_url.as_deref(),
                source.origin_rev.as_deref(),
                source.source_kind.as_str(),
                source.confidence.as_str(),
                source.realization_status.as_str(),
            ],
        )?;
        let source_id = tx.last_insert_rowid();

        if let Some(loc) = &source.loc {
            tx.execute(
                "INSERT INTO source_loc (source_id, policy_hash, counter, loc_total, loc_code, loc_comments, loc_blank) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    source_id,
                    loc.policy_hash,
                    loc.counter,
                    loc.loc_total,
                    loc.loc_code,
                    loc.loc_comments,
                    loc.loc_blank,
                ],
            )?;
            for language in &loc.languages {
                tx.execute(
                    "INSERT INTO source_language_loc (source_id, policy_hash, counter, language, files, loc_total, loc_code, loc_comments, loc_blank) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        source_id,
                        loc.policy_hash,
                        loc.counter,
                        language.language,
                        language.files,
                        language.loc_total,
                        language.loc_code,
                        language.loc_comments,
                        language.loc_blank,
                    ],
                )?;
            }
        }

        for link in &source.links {
            tx.execute(
                "INSERT INTO package_source (path_id, source_id, relationship) VALUES (?1, ?2, ?3)",
                params![path_ids[link.package_path_index], source_id, link.relationship.as_str()],
            )?;
        }
    }
    for dep in &scan.source_dependencies {
        tx.execute(
            "INSERT INTO source_dependency (from_source_id, to_source_id, dependency_kind, dependency_spec) VALUES (?1, ?2, ?3, ?4)",
            params![
                dep.from_source_index as i64 + 1,
                dep.to_source_index as i64 + 1,
                dep.dependency_kind.as_str(),
                dep.dependency_spec,
            ],
        )?;
    }
    for rollup in &scan.source_rollups {
        tx.execute(
            "INSERT INTO source_rollup (source_id, own_code_loc, transitive_code_loc, total_code_loc, unique_transitive_code_loc, shared_transitive_code_loc, reachable_source_count, unique_reachable_source_count, shared_reachable_source_count, runtime_linked, build_time_only) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                rollup.source_index as i64 + 1,
                rollup.own_code_loc,
                rollup.transitive_code_loc,
                rollup.total_code_loc,
                rollup.unique_transitive_code_loc,
                rollup.shared_transitive_code_loc,
                rollup.reachable_source_count as i64,
                rollup.unique_reachable_source_count as i64,
                rollup.shared_reachable_source_count as i64,
                rollup.runtime_linked,
                rollup.build_time_only,
            ],
        )?;
    }
    insert_source_treemap_nodes(tx, scan)?;
    Ok(())
}

fn insert_source_treemap_nodes(tx: &rusqlite::Transaction<'_>, scan: &ScanData) -> Result<()> {
    for view_name in ["source-kind", "ecosystem", "layer"] {
        tx.execute(
            "INSERT INTO source_treemap_node (view_name, node_id, parent_node_id, source_id, label, group_key, color_key, own_code_loc, total_code_loc, unique_transitive_code_loc, shared_transitive_code_loc, reachable_source_count, runtime_linked, build_time_only, source_kind, ecosystem, realization_status) VALUES (?1, ?2, NULL, NULL, ?3, ?3, ?3, 0, 0, 0, 0, 0, 0, 0, ?3, ?3, ?3)",
            params![view_name, 1_i64, view_name],
        )?;

        let mut group_ids = std::collections::BTreeMap::<String, i64>::new();
        let mut next_node_id = 2_i64;
        for (index, source) in scan.sources.iter().enumerate() {
            let rollup = &scan.source_rollups[index];
            let group_key = source_group_key(source, rollup, view_name);
            let group_id = if let Some(group_id) = group_ids.get(&group_key) {
                *group_id
            } else {
                let group_id = next_node_id;
                next_node_id += 1;
                tx.execute(
                    "INSERT INTO source_treemap_node (view_name, node_id, parent_node_id, source_id, label, group_key, color_key, own_code_loc, total_code_loc, unique_transitive_code_loc, shared_transitive_code_loc, reachable_source_count, runtime_linked, build_time_only, source_kind, ecosystem, realization_status) VALUES (?1, ?2, 1, NULL, ?3, ?3, ?3, 0, 0, 0, 0, 0, 0, 0, ?3, ?3, ?3)",
                    params![view_name, group_id, group_key],
                )?;
                group_ids.insert(group_key.clone(), group_id);
                group_id
            };

            tx.execute(
                "INSERT INTO source_treemap_node (view_name, node_id, parent_node_id, source_id, label, group_key, color_key, own_code_loc, total_code_loc, unique_transitive_code_loc, shared_transitive_code_loc, reachable_source_count, runtime_linked, build_time_only, source_kind, ecosystem, realization_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    view_name,
                    next_node_id,
                    group_id,
                    index as i64 + 1,
                    source_display_name(source),
                    group_key,
                    source_color_key(source, rollup, view_name),
                    rollup.own_code_loc,
                    rollup.total_code_loc,
                    rollup.unique_transitive_code_loc,
                    rollup.shared_transitive_code_loc,
                    rollup.reachable_source_count as i64,
                    rollup.runtime_linked,
                    rollup.build_time_only,
                    source.source_kind.as_str(),
                    source.ecosystem.as_str(),
                    source.realization_status.as_str(),
                ],
            )?;
            next_node_id += 1;
        }
    }
    Ok(())
}

fn source_group_key(source: &crate::model::SourceUnit, rollup: &crate::model::SourceRollup, view_name: &str) -> String {
    match view_name {
        "ecosystem" => source.ecosystem.clone(),
        "layer" if rollup.runtime_linked => "runtime".to_string(),
        "layer" => "build".to_string(),
        _ => source.source_kind.to_string(),
    }
}

fn source_color_key(source: &crate::model::SourceUnit, rollup: &crate::model::SourceRollup, view_name: &str) -> String {
    match view_name {
        "ecosystem" => source.ecosystem.clone(),
        "layer" if rollup.runtime_linked => "runtime".to_string(),
        "layer" => "build".to_string(),
        _ => source.source_kind.to_string(),
    }
}

fn source_display_name(source: &crate::model::SourceUnit) -> String {
    source
        .version
        .as_ref()
        .map(|version| format!("{}-{version}", source.name))
        .unwrap_or_else(|| source.name.clone())
}

fn insert_paths(tx: &rusqlite::Transaction<'_>, scan: &ScanData) -> Result<Vec<i64>> {
    let mut path_ids = Vec::with_capacity(scan.nodes.len());
    for node in &scan.nodes {
        tx.execute(
            "INSERT INTO store_path (path, store_hash, name, nar_size, closure_size, deriver_status, deriver_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                node.path,
                node.store_hash,
                node.name,
                node.nar_size,
                node.closure_size,
                node.deriver_status.as_str(),
                node.deriver_path
            ],
        )?;
        path_ids.push(tx.last_insert_rowid());
    }
    Ok(path_ids)
}

fn insert_edges(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    for (from, to) in &scan.edges {
        tx.execute(
            "INSERT INTO runtime_edge (from_path_id, to_path_id) VALUES (?1, ?2)",
            params![path_ids[*from], path_ids[*to]],
        )?;
    }
    Ok(())
}

fn insert_rollups(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    for (index, node) in scan.nodes.iter().enumerate() {
        let nar_size = node.nar_size.unwrap_or(0);
        let shared_by_immediate_parent = scan.graph.reverse_ref_count[index] > 1;
        tx.execute(
            "INSERT INTO runtime_rollup (path_id, runtime_ref_count, reverse_ref_count, added_size, shared_size) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                path_ids[index],
                scan.graph.adjacency[index].len() as i64,
                scan.graph.reverse_ref_count[index] as i64,
                if shared_by_immediate_parent { 0 } else { nar_size },
                if shared_by_immediate_parent { nar_size } else { 0 },
            ],
        )?;
        tx.execute(
            "INSERT INTO ownership_rollup (path_id, immediate_parent_count, top_owner_count, is_unique_to_parent, unique_bytes, shared_bytes, ownership_weight_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                path_ids[index],
                scan.ownership[index].immediate_parent_count as i64,
                scan.ownership[index].top_owner_count as i64,
                scan.ownership[index].top_owner_count == 1,
                scan.ownership[index].unique_bytes,
                scan.ownership[index].shared_bytes,
                scan.ownership[index].ownership_weight_json,
            ],
        )?;
        tx.execute(
            "INSERT INTO path_category (path_id, category, confidence, reason) VALUES (?1, ?2, ?3, ?4)",
            params![
                path_ids[index],
                scan.categories[index].name,
                scan.categories[index].confidence,
                scan.categories[index].reason
            ],
        )?;
    }
    Ok(())
}
