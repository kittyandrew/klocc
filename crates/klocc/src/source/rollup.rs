use crate::model::{SourceDependency, SourceRollup, SourceUnit};

pub(super) fn compute_rollups(units: &[SourceUnit], dependencies: &[SourceDependency]) -> Vec<SourceRollup> {
    let mut adjacency = vec![Vec::new(); units.len()];
    let mut reverse_dependency_count = vec![0usize; units.len()];
    for dependency in dependencies {
        adjacency[dependency.from_source_index].push(dependency.to_source_index);
        reverse_dependency_count[dependency.to_source_index] += 1;
    }

    let mut seen = vec![0usize; units.len()];
    let mut stack = vec![0usize; units.len()];
    let mut reachable = Vec::new();
    let mut rollups = Vec::with_capacity(units.len());

    for source_index in 0..units.len() {
        let stamp = source_index + 1;
        reachable.clear();
        collect_reachable(source_index, &adjacency, &mut seen, &mut stack, &mut reachable, stamp);

        let mut transitive_code_loc = 0;
        let mut unique_transitive_code_loc = 0;
        let mut reachable_source_count = 0;
        let mut unique_reachable_source_count = 0;
        for index in &reachable {
            if *index == source_index {
                continue;
            }
            reachable_source_count += 1;
            let code_loc = units[*index].loc.as_ref().map_or(0, |loc| loc.loc_code);
            transitive_code_loc += code_loc;
            if reverse_dependency_count[*index] <= 1 {
                unique_reachable_source_count += 1;
                unique_transitive_code_loc += code_loc;
            }
        }

        let own_code_loc = units[source_index].loc.as_ref().map_or(0, |loc| loc.loc_code);
        let shared_transitive_code_loc = transitive_code_loc - unique_transitive_code_loc;
        let runtime_linked = !units[source_index].links.is_empty();
        rollups.push(SourceRollup {
            source_index,
            own_code_loc,
            transitive_code_loc,
            total_code_loc: own_code_loc + transitive_code_loc,
            unique_transitive_code_loc,
            shared_transitive_code_loc,
            reachable_source_count,
            unique_reachable_source_count,
            shared_reachable_source_count: reachable_source_count - unique_reachable_source_count,
            runtime_linked,
            build_time_only: !runtime_linked,
        });
    }

    rollups
}

fn collect_reachable(
    source_index: usize,
    adjacency: &[Vec<usize>],
    seen: &mut [usize],
    stack: &mut [usize],
    reachable: &mut Vec<usize>,
    stamp: usize,
) {
    if stack[source_index] == stamp {
        return;
    }
    stack[source_index] = stamp;
    for child in &adjacency[source_index] {
        if seen[*child] != stamp {
            seen[*child] = stamp;
            reachable.push(*child);
            collect_reachable(*child, adjacency, seen, stack, reachable, stamp);
        }
    }
    stack[source_index] = 0;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::model::{SourceConfidence, SourceKind, SourceLink, SourceLoc, SourceRelationship};

    type GraphCase = (usize, Vec<i64>, Vec<bool>, Vec<(usize, usize)>);

    proptest! {
        #[test]
        fn rollups_match_independent_reachability_reference(
            (node_count, locs, runtime_linked, raw_edges) in graph_strategy()
        ) {
            let units = (0..node_count)
                .map(|index| source_unit(index, locs[index], runtime_linked[index]))
                .collect::<Vec<_>>();
            let edges = raw_edges
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|(from, to)| SourceDependency {
                    from_source_index: from,
                    to_source_index: to,
                    dependency_kind: "test".into(),
                    dependency_spec: None,
                })
                .collect::<Vec<_>>();

            let rollups = compute_rollups(&units, &edges);
            let expected = reference_rollups(&locs, &edges);

            prop_assert_eq!(rollups.len(), node_count);
            for (index, rollup) in rollups.iter().enumerate() {
                let expected = &expected[index];
                prop_assert_eq!(rollup.source_index, index);
                prop_assert_eq!(rollup.own_code_loc, locs[index]);
                prop_assert_eq!(rollup.transitive_code_loc, expected.transitive_code_loc);
                prop_assert_eq!(rollup.total_code_loc, locs[index] + expected.transitive_code_loc);
                prop_assert_eq!(rollup.unique_transitive_code_loc, expected.unique_transitive_code_loc);
                prop_assert_eq!(rollup.shared_transitive_code_loc, expected.shared_transitive_code_loc);
                prop_assert_eq!(rollup.reachable_source_count, expected.reachable_source_count);
                prop_assert_eq!(rollup.unique_reachable_source_count, expected.unique_reachable_source_count);
                prop_assert_eq!(rollup.shared_reachable_source_count, expected.shared_reachable_source_count);
                prop_assert_eq!(rollup.runtime_linked, runtime_linked[index]);
                prop_assert_eq!(rollup.build_time_only, !runtime_linked[index]);
                prop_assert_eq!(rollup.transitive_code_loc, rollup.unique_transitive_code_loc + rollup.shared_transitive_code_loc);
                prop_assert_eq!(rollup.reachable_source_count, rollup.unique_reachable_source_count + rollup.shared_reachable_source_count);
                prop_assert!(rollup.reachable_source_count < node_count);
            }
        }
    }

    fn graph_strategy() -> impl Strategy<Value = GraphCase> {
        (1usize..25).prop_flat_map(|node_count| {
            (
                Just(node_count),
                prop::collection::vec(0_i64..10_000, node_count),
                prop::collection::vec(any::<bool>(), node_count),
                prop::collection::vec((0..node_count, 0..node_count), 0..node_count * 4),
            )
        })
    }

    fn source_unit(index: usize, loc_code: i64, runtime_linked: bool) -> SourceUnit {
        SourceUnit {
            name: format!("source-{index}"),
            version: None,
            ecosystem: "test".to_string(),
            source_store_path: Some(format!("/nix/store/test-source-{index}")),
            origin_url: None,
            origin_rev: None,
            source_kind: SourceKind::from("test-source"),
            confidence: SourceConfidence::from("test"),
            realization_status: "realized".into(),
            links: runtime_linked
                .then(|| SourceLink {
                    package_path_index: 0,
                    relationship: SourceRelationship::from("runtime"),
                })
                .into_iter()
                .collect(),
            loc: Some(SourceLoc {
                policy_hash: "test".to_string(),
                counter: "test".to_string(),
                loc_total: loc_code,
                loc_code,
                loc_comments: 0,
                loc_blank: 0,
                languages: Vec::new(),
            }),
        }
    }

    #[derive(Debug)]
    struct ExpectedRollup {
        transitive_code_loc: i64,
        unique_transitive_code_loc: i64,
        shared_transitive_code_loc: i64,
        reachable_source_count: usize,
        unique_reachable_source_count: usize,
        shared_reachable_source_count: usize,
    }

    fn reference_rollups(locs: &[i64], edges: &[SourceDependency]) -> Vec<ExpectedRollup> {
        let mut adjacency = vec![Vec::new(); locs.len()];
        let mut reverse_dependency_count = vec![0usize; locs.len()];
        for edge in edges {
            adjacency[edge.from_source_index].push(edge.to_source_index);
            reverse_dependency_count[edge.to_source_index] += 1;
        }

        (0..locs.len())
            .map(|source_index| {
                let mut seen = BTreeSet::new();
                collect_reference(source_index, source_index, &adjacency, &mut seen, &mut Vec::new());
                let mut transitive_code_loc = 0;
                let mut unique_transitive_code_loc = 0;
                let mut unique_reachable_source_count = 0;
                for reachable in &seen {
                    transitive_code_loc += locs[*reachable];
                    if reverse_dependency_count[*reachable] <= 1 {
                        unique_reachable_source_count += 1;
                        unique_transitive_code_loc += locs[*reachable];
                    }
                }
                let reachable_source_count = seen.len();
                let shared_transitive_code_loc = transitive_code_loc - unique_transitive_code_loc;
                ExpectedRollup {
                    transitive_code_loc,
                    unique_transitive_code_loc,
                    shared_transitive_code_loc,
                    reachable_source_count,
                    unique_reachable_source_count,
                    shared_reachable_source_count: reachable_source_count - unique_reachable_source_count,
                }
            })
            .collect()
    }

    fn collect_reference(
        root: usize,
        source_index: usize,
        adjacency: &[Vec<usize>],
        seen: &mut BTreeSet<usize>,
        stack: &mut Vec<usize>,
    ) {
        if stack.contains(&source_index) {
            return;
        }
        stack.push(source_index);
        for child in &adjacency[source_index] {
            if *child == root || seen.insert(*child) {
                collect_reference(root, *child, adjacency, seen, stack);
            }
        }
        stack.pop();
    }
}
