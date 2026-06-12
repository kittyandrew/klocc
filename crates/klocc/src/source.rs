use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use crate::model::{
    DerivationNode, DerivationSourceLink, SourceGraph, SourceLink, SourceScanStats, SourceUnit, StoreNode,
};
use crate::nix::NixRunner;

mod builder;
mod candidates;
mod cargo_graph;
mod derivation_json;
mod loc;
mod naming;
mod rollup;

use builder::{SourceBuilder, add_generated_derivation_source, add_unknown_derivation_source};
use candidates::SourceCandidate;
use cargo_graph::add_cargo_graph;
use derivation_json::{DerivationInfo, parse_derivation_show};
use loc::{LocCache, LocMeasureContext, SourceMeasurement, measure_source_path};
use naming::{StartsWithStore, package_name_from_store_name, version_from_store_name};
use rollup::compute_rollups;

struct NixGraphTraversal<'a> {
    runner: &'a mut NixRunner,
    derivation_cache: &'a mut BTreeMap<String, DerivationInfo>,
    drv_source_indices: &'a mut HashMap<String, Vec<usize>>,
    expanded_drvs: BTreeSet<String>,
    recursive_loaded: &'a mut BTreeSet<String>,
    loc_cache: &'a mut HashMap<String, SourceMeasurement>,
    persistent_loc_cache: Option<&'a mut LocCache>,
    loc_stats: &'a mut SourceScanStats,
}

pub fn collect(
    root_input: &str,
    nodes: &[StoreNode],
    root_index: usize,
    runner: &mut NixRunner,
    root_derivations: Option<&Value>,
) -> Result<SourceGraph> {
    let phase_started = Instant::now();
    let mut builder = SourceBuilder::default();
    let root_name = package_name_from_store_name(&nodes[root_index].name);
    let mut root_source_index = None;
    let mut root_source_path = local_flake_source_root(root_input)?;
    let mut vendor_paths = Vec::new();
    let mut derivation_cache = BTreeMap::new();
    let mut drv_source_indices = HashMap::<String, Vec<usize>>::new();
    let mut loc_cache = HashMap::<String, SourceMeasurement>::new();
    let mut persistent_loc_cache = LocCache::open().ok();
    let mut loc_stats = SourceScanStats::default();
    let mut graph_roots = Vec::new();
    let mut recursive_loaded = match root_derivations {
        Some(value) => cache_derivation_value(value, &mut derivation_cache),
        None => cache_recursive_derivations(root_input, runner, &mut derivation_cache),
    };
    crate::time::log_timing("source", "init_derivations", phase_started.elapsed());

    let phase_started = Instant::now();
    for (package_index, node) in nodes.iter().enumerate() {
        let Some(deriver_path) = &node.deriver_path else {
            add_unknown_source(&mut builder, package_index, node)?;
            continue;
        };

        let info = match derivation_info(deriver_path, runner, &mut derivation_cache) {
            Ok(info) => info.clone(),
            Err(_) => {
                let source_index = add_derivation_unavailable_source(&mut builder, package_index, node);
                builder.add_derivation_source_link(deriver_path, source_index, "derivation-unavailable");
                drv_source_indices.insert(deriver_path.clone(), vec![source_index]);
                continue;
            }
        };

        let mut node_source_indices = Vec::new();
        for candidate in info.source_candidates.clone() {
            let path = PathBuf::from(&candidate.path);
            if candidate.relationship == "package-source" && package_index == root_index {
                root_source_path = Some(path.clone());
            }
            if candidate.relationship == "vendored-source" {
                vendor_paths.push(path.clone());
                continue;
            }

            let (name, version, ecosystem) = if package_index == root_index {
                (
                    root_name.clone(),
                    version_from_store_name(&node.name),
                    "cargo-workspace".to_string(),
                )
            } else {
                (
                    package_name_from_store_name(&node.name),
                    version_from_store_name(&node.name),
                    "nix".to_string(),
                )
            };
            let measurement = measure_source_path(
                &path,
                runner,
                &mut loc_cache,
                persistent_loc_cache.as_mut(),
                &mut loc_stats,
            )?;
            let relationship = candidate.relationship.clone();
            let index = builder.add_unit(SourceUnit {
                name,
                version,
                ecosystem,
                source_store_path: candidate
                    .path
                    .starts_with("/nix/store/")
                    .then(|| candidate.path.clone()),
                origin_url: (!candidate.path.starts_with("/nix/store/")).then(|| format!("file://{}", candidate.path)),
                origin_rev: None,
                source_kind: candidate.source_kind.into(),
                confidence: candidate.confidence.into(),
                realization_status: measurement.realization_status,
                links: vec![SourceLink {
                    package_path_index: package_index,
                    relationship: relationship.clone().into(),
                }],
                loc: measurement.loc,
            });
            builder.add_derivation_source_link(deriver_path, index, &relationship);
            node_source_indices.push(index);
            if package_index == root_index {
                root_source_index = Some(index);
            }
        }
        if node_source_indices.is_empty() {
            let (source_index, relationship) = if info.input_drvs.is_empty() {
                add_no_source_candidate(&mut builder, package_index, node)
            } else {
                add_generated_source(&mut builder, package_index, node)
            };
            builder.add_derivation_source_link(deriver_path, source_index, relationship);
            drv_source_indices.insert(deriver_path.clone(), vec![source_index]);
            graph_roots.push(deriver_path.clone());
        } else {
            drv_source_indices.insert(deriver_path.clone(), node_source_indices.clone());
            graph_roots.push(deriver_path.clone());
        }
    }

    let mut traversal = NixGraphTraversal {
        runner,
        derivation_cache: &mut derivation_cache,
        drv_source_indices: &mut drv_source_indices,
        expanded_drvs: BTreeSet::new(),
        recursive_loaded: &mut recursive_loaded,
        loc_cache: &mut loc_cache,
        persistent_loc_cache: persistent_loc_cache.as_mut(),
        loc_stats: &mut loc_stats,
    };
    for deriver_path in graph_roots {
        collect_nix_dependency_graph(&mut builder, &deriver_path, &mut traversal)?;
    }
    crate::time::log_timing("source", "nix_graph", phase_started.elapsed());

    let phase_started = Instant::now();
    if root_source_index.is_none()
        && let Some(path) = root_source_path.clone()
    {
        let measurement = measure_source_path(
            &path,
            runner,
            &mut loc_cache,
            persistent_loc_cache.as_mut(),
            &mut loc_stats,
        )?;
        root_source_index = Some(builder.add_unit(SourceUnit {
            name: root_name.clone(),
            version: version_from_store_name(&nodes[root_index].name),
            ecosystem: "cargo-workspace".to_string(),
            source_store_path: path.starts_with("/nix/store/").then(|| path.display().to_string()),
            origin_url: (!path.starts_with("/nix/store/")).then(|| format!("file://{}", path.display())),
            origin_rev: None,
            source_kind: "local-flake-source-fallback".into(),
            confidence: "medium".into(),
            realization_status: measurement.realization_status,
            links: vec![SourceLink {
                package_path_index: root_index,
                relationship: "package-source".into(),
            }],
            loc: measurement.loc,
        }));
    }
    crate::time::log_timing("source", "root_source", phase_started.elapsed());

    let phase_started = Instant::now();
    if let (Some(root_source_path), Some(root_source_index)) = (root_source_path.as_ref(), root_source_index) {
        let mut loc_context = LocMeasureContext {
            runner,
            loc_cache: &mut loc_cache,
            persistent_loc_cache: persistent_loc_cache.as_mut(),
            loc_stats: &mut loc_stats,
        };
        add_cargo_graph(
            &mut builder,
            root_source_path,
            &vendor_paths,
            &root_name,
            root_source_index,
            &mut loc_context,
        )?;
    }
    crate::time::log_timing("source", "cargo_graph", phase_started.elapsed());

    let phase_started = Instant::now();
    let rollups = compute_rollups(&builder.units, &builder.dependencies);
    crate::time::log_timing("source", "rollups", phase_started.elapsed());

    let phase_started = Instant::now();
    let derivations = build_derivation_nodes(&derivation_cache, &builder.derivation_source_links);
    crate::time::log_timing("source", "derivation_nodes", phase_started.elapsed());

    Ok(SourceGraph {
        units: builder.units,
        dependencies: builder.dependencies,
        rollups,
        derivations,
        stats: loc_stats,
    })
}

fn collect_nix_dependency_graph(
    builder: &mut SourceBuilder,
    drv_path: &str,
    traversal: &mut NixGraphTraversal<'_>,
) -> Result<()> {
    let started = Instant::now();
    let mut last_progress = started;
    let mut queue = vec![drv_path.to_string()];
    while let Some(current_drv) = queue.pop() {
        if !traversal.expanded_drvs.insert(current_drv.clone()) {
            continue;
        }
        if !traversal.recursive_loaded.contains(&current_drv) {
            let loaded = cache_recursive_derivations(&current_drv, traversal.runner, traversal.derivation_cache);
            traversal.recursive_loaded.extend(loaded);
        }
        let parent_sources = ensure_derivation_sources(builder, &current_drv, traversal)?;
        let info = derivation_info(&current_drv, traversal.runner, traversal.derivation_cache)?.clone();

        for source_candidate in info.input_srcs {
            let child_index = add_candidate_source(
                builder,
                source_candidate,
                None,
                traversal.runner,
                traversal.loc_cache,
                traversal.persistent_loc_cache.as_deref_mut(),
                traversal.loc_stats,
            )?;
            for parent in &parent_sources {
                builder.add_dependency(*parent, child_index, "nix:source-input", None);
            }
        }

        for input_drv in info.input_drvs {
            let child_sources = ensure_derivation_sources(builder, &input_drv.input_drv_path, traversal)?;
            for parent in &parent_sources {
                for child in &child_sources {
                    builder.add_dependency(*parent, *child, "nix:build", Some(input_drv.input_drv_path.clone()));
                }
            }
            queue.push(input_drv.input_drv_path);
        }
        log_nix_graph_progress(builder, traversal, queue.len(), started, &mut last_progress);
    }
    Ok(())
}

fn log_nix_graph_progress(
    builder: &SourceBuilder,
    traversal: &NixGraphTraversal<'_>,
    queued_drvs: usize,
    started: Instant,
    last_progress: &mut Instant,
) {
    if !crate::time::timings_enabled() || last_progress.elapsed() < Duration::from_secs(5) {
        return;
    }

    eprintln!(
        "klocc: progress source.nix_graph expanded_drvs={} queued_drvs={} source_units={} source_edges={} elapsed={:.3}ms",
        traversal.expanded_drvs.len(),
        queued_drvs,
        builder.units.len(),
        builder.dependencies.len(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
    *last_progress = Instant::now();
}

fn ensure_derivation_sources(
    builder: &mut SourceBuilder,
    drv_path: &str,
    traversal: &mut NixGraphTraversal<'_>,
) -> Result<Vec<usize>> {
    if let Some(source_indices) = traversal.drv_source_indices.get(drv_path) {
        return Ok(source_indices.clone());
    }

    let info = derivation_info(drv_path, traversal.runner, traversal.derivation_cache)?.clone();
    let candidates = if info.source_candidates.is_empty() {
        info.input_srcs
    } else {
        info.source_candidates
    };
    let mut source_indices = Vec::new();
    for candidate in candidates {
        let source_index = add_candidate_source(
            builder,
            candidate.clone(),
            None,
            traversal.runner,
            traversal.loc_cache,
            traversal.persistent_loc_cache.as_deref_mut(),
            traversal.loc_stats,
        )?;
        builder.add_derivation_source_link(drv_path, source_index, &candidate.relationship);
        source_indices.push(source_index);
    }
    if source_indices.is_empty() {
        let (source_index, relationship) = if info.input_drvs.is_empty() {
            (add_unknown_derivation_source(builder, drv_path), "unknown-source")
        } else {
            (add_generated_derivation_source(builder, drv_path), "generated-output")
        };
        builder.add_derivation_source_link(drv_path, source_index, relationship);
        source_indices.push(source_index);
    }
    source_indices.sort_unstable();
    source_indices.dedup();
    traversal
        .drv_source_indices
        .insert(drv_path.to_string(), source_indices.clone());
    Ok(source_indices)
}

fn add_candidate_source(
    builder: &mut SourceBuilder,
    candidate: SourceCandidate,
    package_link: Option<SourceLink>,
    runner: &mut NixRunner,
    loc_cache: &mut HashMap<String, SourceMeasurement>,
    persistent_loc_cache: Option<&mut LocCache>,
    loc_stats: &mut SourceScanStats,
) -> Result<usize> {
    let path = PathBuf::from(&candidate.path);
    let store_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(candidate.path.as_str());
    let name = package_name_from_store_name(store_name);
    let measurement = measure_source_path(&path, runner, loc_cache, persistent_loc_cache, loc_stats)?;
    Ok(builder.add_unit(SourceUnit {
        name,
        version: version_from_store_name(store_name),
        ecosystem: "nix".to_string(),
        source_store_path: candidate
            .path
            .starts_with("/nix/store/")
            .then(|| candidate.path.clone()),
        origin_url: (!candidate.path.starts_with("/nix/store/")).then(|| format!("file://{}", candidate.path)),
        origin_rev: None,
        source_kind: candidate.source_kind.into(),
        confidence: candidate.confidence.into(),
        realization_status: measurement.realization_status,
        links: package_link.into_iter().collect(),
        loc: measurement.loc,
    }))
}

fn derivation_info(
    drv_path: &str,
    runner: &mut NixRunner,
    cache: &mut BTreeMap<String, DerivationInfo>,
) -> Result<DerivationInfo> {
    if let Some(info) = cache.get(drv_path) {
        return Ok(info.clone());
    }

    let value = runner.derivation_show(drv_path)?;
    let mut infos = parse_derivation_show(&value)?;
    let info = infos.remove(drv_path).unwrap_or_else(|| DerivationInfo {
        drv_path: drv_path.to_string(),
        name: None,
        system: None,
        builder: None,
        is_fixed_output: false,
        raw_json: "null".to_string(),
        outputs: Vec::new(),
        input_drvs: Vec::new(),
        input_srcs: Vec::new(),
        source_candidates: Vec::new(),
    });
    cache.insert(drv_path.to_string(), info.clone());
    Ok(info)
}

fn cache_recursive_derivations(
    root: &str,
    runner: &mut NixRunner,
    cache: &mut BTreeMap<String, DerivationInfo>,
) -> BTreeSet<String> {
    let Ok(value) = runner.derivation_show_recursive(root) else {
        return BTreeSet::new();
    };
    cache_derivation_value(&value, cache)
}

fn cache_derivation_value(value: &Value, cache: &mut BTreeMap<String, DerivationInfo>) -> BTreeSet<String> {
    let Ok(infos) = parse_derivation_show(value) else {
        return BTreeSet::new();
    };
    let mut loaded = BTreeSet::new();
    for (drv_path, info) in infos {
        loaded.insert(drv_path.clone());
        cache.entry(drv_path).or_insert(info);
    }
    loaded
}

fn build_derivation_nodes(
    cache: &BTreeMap<String, DerivationInfo>,
    source_links: &HashMap<String, Vec<DerivationSourceLink>>,
) -> Vec<DerivationNode> {
    cache
        .values()
        .map(|info| DerivationNode {
            drv_path: info.drv_path.clone(),
            name: info.name.clone(),
            system: info.system.clone(),
            builder: info.builder.clone(),
            is_fixed_output: info.is_fixed_output,
            raw_json: info.raw_json.clone(),
            outputs: info.outputs.clone(),
            input_derivations: info.input_drvs.clone(),
            input_sources: info.input_srcs.iter().map(|source| source.path.clone()).collect(),
            source_links: source_links.get(&info.drv_path).cloned().unwrap_or_default(),
        })
        .collect()
}

fn add_unknown_source(builder: &mut SourceBuilder, package_index: usize, node: &StoreNode) -> Result<()> {
    builder.add_unit(SourceUnit {
        name: package_name_from_store_name(&node.name),
        version: version_from_store_name(&node.name),
        ecosystem: "nix".to_string(),
        source_store_path: None,
        origin_url: None,
        origin_rev: None,
        source_kind: "unknown-source".into(),
        confidence: "low".into(),
        realization_status: "unknown-deriver".into(),
        links: vec![SourceLink {
            package_path_index: package_index,
            relationship: "unknown-source".into(),
        }],
        loc: None,
    });
    Ok(())
}

fn add_derivation_unavailable_source(builder: &mut SourceBuilder, package_index: usize, node: &StoreNode) -> usize {
    builder.add_unit(SourceUnit {
        name: package_name_from_store_name(&node.name),
        version: version_from_store_name(&node.name),
        ecosystem: "nix".to_string(),
        source_store_path: None,
        origin_url: None,
        origin_rev: None,
        source_kind: "unknown-derivation-source".into(),
        confidence: "low".into(),
        realization_status: "derivation-unavailable".into(),
        links: vec![SourceLink {
            package_path_index: package_index,
            relationship: "derivation-unavailable".into(),
        }],
        loc: None,
    })
}

fn add_generated_source(builder: &mut SourceBuilder, package_index: usize, node: &StoreNode) -> (usize, &'static str) {
    let index = builder.add_unit(SourceUnit {
        name: package_name_from_store_name(&node.name),
        version: version_from_store_name(&node.name),
        ecosystem: "nix".to_string(),
        source_store_path: None,
        origin_url: None,
        origin_rev: None,
        source_kind: "generated-derivation-output".into(),
        confidence: "medium".into(),
        realization_status: "generated-from-inputs".into(),
        links: vec![SourceLink {
            package_path_index: package_index,
            relationship: "generated-output".into(),
        }],
        loc: None,
    });
    (index, "generated-output")
}

fn add_no_source_candidate(
    builder: &mut SourceBuilder,
    package_index: usize,
    node: &StoreNode,
) -> (usize, &'static str) {
    let index = builder.add_unit(SourceUnit {
        name: package_name_from_store_name(&node.name),
        version: version_from_store_name(&node.name),
        ecosystem: "nix".to_string(),
        source_store_path: None,
        origin_url: None,
        origin_rev: None,
        source_kind: "unknown-derivation-source".into(),
        confidence: "low".into(),
        realization_status: "no-source-candidate".into(),
        links: vec![SourceLink {
            package_path_index: package_index,
            relationship: "unknown-source".into(),
        }],
        loc: None,
    });
    (index, "unknown-source")
}

fn local_flake_source_root(root_input: &str) -> Result<Option<PathBuf>> {
    let installable = root_input.split_once('#').map_or(root_input, |(path, _)| path);
    let path = if installable.is_empty() || installable == "." {
        Some(std::env::current_dir()?)
    } else if let Some(path) = installable.strip_prefix("path:") {
        Some(PathBuf::from(path))
    } else if installable.starts_with('/') && !installable.starts_with("/nix/store/") {
        Some(PathBuf::from(installable))
    } else {
        None
    };

    Ok(path.and_then(|path| path.canonicalize().ok()))
}

#[cfg(test)]
mod tests {
    use super::candidates::{source_candidate_path_from_reference, source_like_name};
    use super::*;
    use crate::model::{SourceDependency, SourceLoc};

    fn source(name: &str, code: i64) -> SourceUnit {
        SourceUnit {
            name: name.to_string(),
            version: None,
            ecosystem: "test".to_string(),
            source_store_path: None,
            origin_url: None,
            origin_rev: None,
            source_kind: "test".into(),
            confidence: "high".into(),
            realization_status: "available".into(),
            links: Vec::new(),
            loc: Some(SourceLoc {
                policy_hash: "test".to_string(),
                counter: "test".to_string(),
                loc_total: code,
                loc_code: code,
                loc_comments: 0,
                loc_blank: 0,
                languages: Vec::new(),
            }),
        }
    }

    #[test]
    fn rollups_do_not_count_self_reachable_cycles_as_transitive_loc() {
        let units = vec![source("a", 10), source("b", 20)];
        let dependencies = vec![
            SourceDependency {
                from_source_index: 0,
                to_source_index: 1,
                dependency_kind: "test".into(),
                dependency_spec: None,
            },
            SourceDependency {
                from_source_index: 1,
                to_source_index: 0,
                dependency_kind: "test".into(),
                dependency_spec: None,
            },
        ];

        let rollups = compute_rollups(&units, &dependencies);

        assert_eq!(rollups[0].own_code_loc, 10);
        assert_eq!(rollups[0].transitive_code_loc, 20);
        assert_eq!(rollups[0].total_code_loc, 30);
        assert_eq!(rollups[0].reachable_source_count, 1);

        assert_eq!(rollups[1].own_code_loc, 20);
        assert_eq!(rollups[1].transitive_code_loc, 10);
        assert_eq!(rollups[1].total_code_loc, 30);
        assert_eq!(rollups[1].reachable_source_count, 1);
    }

    #[test]
    fn embedded_store_source_references_collapse_to_source_root() {
        assert_eq!(
            source_candidate_path_from_reference(
                "/nix/store/4f7nm8dblrdy74gmsl62b25rjrh4jcz3-stage0-posix-1.9.1-source/M2libc/amd64/libc-full.M1",
            )
            .as_deref(),
            Some("/nix/store/4f7nm8dblrdy74gmsl62b25rjrh4jcz3-stage0-posix-1.9.1-source"),
        );
    }

    #[test]
    fn source_like_names_include_common_nix_source_archives() {
        assert!(source_like_name("Python-3.14.2.tar.xz"));
        assert!(source_like_name("foo-1.0.tar.bz2"));
    }
}
