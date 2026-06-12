use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::category;
use crate::db::writer;
use crate::graph;
use crate::model::{ScanData, ScanHealthMetric};
use crate::nix::NixRunner;
use crate::ownership;
use crate::source;
use crate::stats;

pub fn run(root_input: &str, out: &Path) -> Result<()> {
    if out.exists() {
        bail!("output artifact already exists: {}", out.display());
    }

    let scan_started = Instant::now();
    let mut runner = NixRunner::new();
    let phase_started = Instant::now();
    let nix_version = runner.nix_version()?;
    let resolved_root = runner.realize(root_input)?;
    let root_infos = runner.path_info(&resolved_root, false)?;
    let root_store_path = root_infos
        .first()
        .map(|node| node.path.clone())
        .ok_or_else(|| anyhow!("nix path-info returned no root path for {root_input}"))?;
    crate::time::log_timing("scan", "resolve_root", phase_started.elapsed());

    let phase_started = Instant::now();
    let mut nodes = runner.path_info(&resolved_root, true)?;
    nodes.sort_by(|left, right| left.path.cmp(&right.path));
    nodes.dedup_by(|left, right| left.path == right.path);
    if !nodes.iter().any(|node| node.path == root_store_path) {
        bail!("resolved root path {root_store_path} was not present in recursive closure output");
    }
    crate::time::log_timing("scan", "runtime_closure", phase_started.elapsed());

    let phase_started = Instant::now();
    if nodes.iter().any(|node| node.deriver_status == "not-queried") {
        runner.fill_derivers(&mut nodes)?;
    }
    let root_derivations = runner.derivation_show_recursive(root_input).ok();
    apply_eval_deriver_fallback(&mut nodes, root_derivations.as_ref());
    crate::time::log_timing("scan", "derivers", phase_started.elapsed());

    let phase_started = Instant::now();
    let path_to_index: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.path.clone(), index))
        .collect();
    let closure_paths: BTreeSet<String> = nodes.iter().map(|node| node.path.clone()).collect();
    let mut edges = Vec::new();

    for node in &nodes {
        let from = *path_to_index
            .get(&node.path)
            .ok_or_else(|| anyhow!("internal error: missing path index for {}", node.path))?;
        let references = if node.references_queried {
            node.references.clone()
        } else {
            runner.query_references(&node.path)?
        };
        for reference in references {
            if closure_paths.contains(&reference) {
                let to = *path_to_index
                    .get(&reference)
                    .ok_or_else(|| anyhow!("internal error: missing path index for {reference}"))?;
                if from != to {
                    edges.push((from, to));
                }
            }
        }
    }

    edges.sort_unstable();
    edges.dedup();
    crate::time::log_timing("scan", "runtime_edges", phase_started.elapsed());

    let phase_started = Instant::now();
    let runtime_graph = graph::build(nodes.len(), &edges);
    let root_index = *path_to_index
        .get(&root_store_path)
        .ok_or_else(|| anyhow!("internal error: missing resolved root path index for {root_store_path}"))?;
    let ownership = ownership::compute(
        &nodes,
        &runtime_graph.adjacency,
        &runtime_graph.reverse_ref_count,
        root_index,
    )?;
    let categories = nodes.iter().map(|node| category::classify(&node.name)).collect();
    crate::time::log_timing("scan", "runtime_rollups", phase_started.elapsed());

    let phase_started = Instant::now();
    let source_graph = source::collect(root_input, &nodes, root_index, &mut runner, root_derivations.as_ref())?;
    crate::time::log_timing("scan", "source_graph", phase_started.elapsed());

    let phase_started = Instant::now();
    let health_metrics = build_health_metrics(&nodes, &source_graph);

    let scan_data = ScanData {
        root_input: root_input.to_string(),
        root_store_path,
        nix_version,
        commands: runner.into_commands(),
        nodes,
        edges,
        graph: runtime_graph,
        ownership,
        categories,
        sources: source_graph.units,
        source_dependencies: source_graph.dependencies,
        source_rollups: source_graph.rollups,
        derivations: source_graph.derivations,
        health_metrics,
        root_index,
    };
    writer::write(out, &scan_data)?;
    crate::time::log_timing("scan", "write", phase_started.elapsed());
    crate::time::log_timing("scan", "total", scan_started.elapsed());

    println!("wrote {}", out.display());
    stats::run(out)
}

fn build_health_metrics(
    nodes: &[crate::model::StoreNode],
    source_graph: &crate::model::SourceGraph,
) -> Vec<ScanHealthMetric> {
    let derivation_inputs = source_graph
        .derivations
        .iter()
        .map(|derivation| derivation.input_derivations.len() as i64)
        .sum();
    let derivation_source_inputs = source_graph
        .derivations
        .iter()
        .map(|derivation| derivation.input_sources.len() as i64)
        .sum();
    let derivation_source_links = source_graph
        .derivations
        .iter()
        .map(|derivation| derivation.source_links.len() as i64)
        .sum();
    let sources_with_loc = source_graph.units.iter().filter(|source| source.loc.is_some()).count() as i64;
    let unknown_derivation_sources = source_graph
        .units
        .iter()
        .filter(|source| source.source_kind == "unknown-derivation-source")
        .count() as i64;
    let generated_derivation_outputs = source_graph
        .units
        .iter()
        .filter(|source| source.source_kind == "generated-derivation-output")
        .count() as i64;
    let unknown_derivers = nodes
        .iter()
        .filter(|node| node.deriver_status == "unknown-deriver")
        .count() as i64;
    let runtime_linked_sources = source_graph
        .rollups
        .iter()
        .filter(|rollup| rollup.runtime_linked)
        .count() as i64;
    let build_time_only_sources = source_graph
        .rollups
        .iter()
        .filter(|rollup| rollup.build_time_only)
        .count() as i64;
    let source_edges = source_graph.dependencies.len() as i64;
    vec![
        metric("runtime_paths", nodes.len() as i64),
        metric("unknown_runtime_derivers", unknown_derivers),
        metric("source_units", source_graph.units.len() as i64),
        metric("source_units_with_loc", sources_with_loc),
        metric("runtime_linked_source_units", runtime_linked_sources),
        metric("build_time_only_source_units", build_time_only_sources),
        metric("unknown_derivation_sources", unknown_derivation_sources),
        metric("generated_derivation_outputs", generated_derivation_outputs),
        metric("source_dependency_edges", source_edges),
        metric("derivations", source_graph.derivations.len() as i64),
        metric("derivation_input_edges", derivation_inputs),
        metric("derivation_source_inputs", derivation_source_inputs),
        metric("derivation_source_unit_links", derivation_source_links),
        metric("loc_memory_cache_hits", source_graph.stats.loc_memory_cache_hits),
        metric(
            "loc_persistent_cache_hits",
            source_graph.stats.loc_persistent_cache_hits,
        ),
        metric("loc_cache_misses", source_graph.stats.loc_cache_misses),
        metric("loc_cache_stores", source_graph.stats.loc_cache_stores),
    ]
}

fn metric(name: &str, value: i64) -> ScanHealthMetric {
    ScanHealthMetric {
        name: name.to_string(),
        value,
    }
}

fn apply_eval_deriver_fallback(nodes: &mut [crate::model::StoreNode], value: Option<&Value>) {
    if nodes.iter().all(|node| node.deriver_status == "known") {
        return;
    }
    let Some(value) = value else {
        return;
    };
    let output_map = derivation_outputs(value);
    for node in nodes {
        if node.deriver_status == "known" {
            continue;
        }
        if let Some(deriver_path) = output_map.get(&node.path) {
            node.deriver_status = "eval-known".into();
            node.deriver_path = Some(deriver_path.clone());
        }
    }
}

fn derivation_outputs(value: &Value) -> std::collections::HashMap<String, String> {
    let mut output_map = std::collections::HashMap::new();
    let Some(derivations) = value
        .get("derivations")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
    else {
        return output_map;
    };
    for (drv_path, drv_value) in derivations {
        if drv_path == "version" {
            continue;
        }
        let Some(outputs) = drv_value.get("outputs").and_then(Value::as_object) else {
            continue;
        };
        for output in outputs.values() {
            if let Some(path) = output.get("path").and_then(Value::as_str) {
                let normalized_output_path = if path.starts_with("/nix/store/") {
                    path.to_string()
                } else {
                    format!("/nix/store/{path}")
                };
                let normalized_drv_path = if drv_path.starts_with("/nix/store/") {
                    drv_path.clone()
                } else {
                    format!("/nix/store/{drv_path}")
                };
                output_map.insert(normalized_output_path, normalized_drv_path);
            }
        }
    }
    output_map
}
