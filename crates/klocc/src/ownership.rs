use anyhow::Result;
use serde_json::json;

use crate::graph::{owner_roots, reachable_from};
use crate::model::{OwnershipRow, StoreNode};

pub fn compute(
    nodes: &[StoreNode],
    adjacency: &[Vec<usize>],
    reverse_ref_count: &[usize],
    root_index: usize,
) -> Result<Vec<OwnershipRow>> {
    let roots = owner_roots(root_index, adjacency);
    let mut owners_by_node: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];

    for (owner_number, owner_root) in roots.iter().enumerate() {
        let reachable = if *owner_root == root_index {
            vec![root_index]
        } else {
            reachable_from(*owner_root, adjacency)
        };
        for node_index in reachable {
            owners_by_node[node_index].push(owner_number);
        }
    }

    let mut rows = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let owner_paths: Vec<_> = owners_by_node[index]
            .iter()
            .map(|owner_number| nodes[roots[*owner_number]].path.as_str())
            .collect();
        let top_owner_count = owner_paths.len();
        let nar_size = node.nar_size.unwrap_or(0);
        rows.push(OwnershipRow {
            immediate_parent_count: reverse_ref_count[index],
            top_owner_count,
            unique_bytes: if top_owner_count == 1 { nar_size } else { 0 },
            shared_bytes: if top_owner_count > 1 { nar_size } else { 0 },
            ownership_weight_json: serde_json::to_string(&json!({
                "policy": "root-only-plus-root-direct-reference-reachability",
                "approximation": "The root bucket owns only the root path; each direct runtime reference bucket owns its transitive runtime closure.",
                "owner_paths": owner_paths,
            }))?,
        });
    }

    Ok(rows)
}
