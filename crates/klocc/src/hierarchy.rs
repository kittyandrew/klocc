use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use rusqlite::params;

use crate::graph::{owner_roots, reachable_from};
use crate::model::{ScanData, StoreNode};

pub fn insert(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    insert_hierarchy_headers(tx)?;
    insert_root_direct_reference(tx, scan, path_ids)?;
    insert_category(tx, scan, path_ids)?;
    Ok(())
}

fn insert_hierarchy_headers(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO hierarchy (hierarchy_id, name, description, metric_default) VALUES (?1, ?2, ?3, ?4)",
        params![
            1,
            "root-direct-reference",
            "Root path, direct runtime reference buckets, and each bucket's transitive runtime closure",
            "nar_size"
        ],
    )?;
    tx.execute(
        "INSERT INTO hierarchy (hierarchy_id, name, description, metric_default) VALUES (?1, ?2, ?3, ?4)",
        params![
            2,
            "category",
            "Phase-1 heuristic category buckets by store path name",
            "nar_size"
        ],
    )?;
    Ok(())
}

fn insert_root_direct_reference(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    let mut next_node_id = 1_i64;
    insert_node(
        tx,
        NodeInsert {
            hierarchy_id: 1,
            node_id: next_node_id,
            parent_node_id: None,
            label: "root",
            path_id: Some(path_ids[scan.root_index]),
            metric: Metric::single(&scan.nodes[scan.root_index], 1),
            color: "root",
        },
    )?;
    let root_node_id = next_node_id;
    next_node_id += 1;

    let owner_root_indexes: Vec<_> = owner_roots(scan.root_index, &scan.graph.adjacency)
        .into_iter()
        .filter(|owner_root| *owner_root != scan.root_index)
        .collect();
    let owner_sets: Vec<_> = owner_root_indexes
        .iter()
        .map(|owner_root| {
            reachable_from(*owner_root, &scan.graph.adjacency)
                .into_iter()
                .collect::<BTreeSet<_>>()
        })
        .collect();
    let owner_counts = owner_counts(scan.nodes.len(), &owner_sets);

    for (owner_number, owner_root) in owner_root_indexes.iter().enumerate() {
        let bucket_id = next_node_id;
        next_node_id += 1;
        let bucket_metric = Metric::aggregate(owner_sets[owner_number].iter().copied(), &scan.nodes, &owner_counts);
        insert_node(
            tx,
            NodeInsert {
                hierarchy_id: 1,
                node_id: bucket_id,
                parent_node_id: Some(root_node_id),
                label: &scan.nodes[*owner_root].name,
                path_id: Some(path_ids[*owner_root]),
                metric: bucket_metric,
                color: "owner-bucket",
            },
        )?;

        for reachable in &owner_sets[owner_number] {
            if *reachable == *owner_root {
                continue;
            }
            let owner_count = owner_counts[*reachable];
            insert_node(
                tx,
                NodeInsert {
                    hierarchy_id: 1,
                    node_id: next_node_id,
                    parent_node_id: Some(bucket_id),
                    label: &scan.nodes[*reachable].name,
                    path_id: Some(path_ids[*reachable]),
                    metric: Metric::single(&scan.nodes[*reachable], owner_count),
                    color: "store-path",
                },
            )?;
            next_node_id += 1;
        }
    }
    Ok(())
}

fn insert_category(tx: &rusqlite::Transaction<'_>, scan: &ScanData, path_ids: &[i64]) -> Result<()> {
    let mut next_node_id = 1_i64;
    tx.execute(
        "INSERT INTO hierarchy_node (hierarchy_id, node_id, parent_node_id, label, path_id, metric_role, nar_size, closure_size, unique_bytes, shared_bytes, attributed_bytes, member_count, duplicate_count, color_category) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![2, next_node_id, Option::<i64>::None, "category", Option::<i64>::None, "root", 0, Option::<i64>::None, 0, 0, 0.0, 0, 0, "root"],
    )?;
    let category_root_id = next_node_id;
    next_node_id += 1;

    let mut by_category: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, category) in scan.categories.iter().enumerate() {
        by_category.entry(category.name.clone()).or_default().push(index);
    }

    let category_counts = vec![1; scan.nodes.len()];
    for (category_name, indexes) in by_category {
        let bucket_id = next_node_id;
        next_node_id += 1;
        let bucket_metric = Metric::aggregate(indexes.iter().copied(), &scan.nodes, &category_counts);
        tx.execute(
            "INSERT INTO hierarchy_node (hierarchy_id, node_id, parent_node_id, label, path_id, metric_role, nar_size, closure_size, unique_bytes, shared_bytes, attributed_bytes, member_count, duplicate_count, color_category) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                2,
                bucket_id,
                category_root_id,
                &category_name,
                Option::<i64>::None,
                "category-bucket",
                bucket_metric.nar_size,
                Option::<i64>::None,
                bucket_metric.unique_bytes,
                bucket_metric.shared_bytes,
                bucket_metric.attributed_bytes,
                bucket_metric.member_count as i64,
                bucket_metric.duplicate_count as i64,
                &category_name,
            ],
        )?;

        for index in indexes {
            insert_node(
                tx,
                NodeInsert {
                    hierarchy_id: 2,
                    node_id: next_node_id,
                    parent_node_id: Some(bucket_id),
                    label: &scan.nodes[index].name,
                    path_id: Some(path_ids[index]),
                    metric: Metric::single(&scan.nodes[index], 1),
                    color: &scan.categories[index].name,
                },
            )?;
            next_node_id += 1;
        }
    }

    Ok(())
}

struct NodeInsert<'a> {
    hierarchy_id: i64,
    node_id: i64,
    parent_node_id: Option<i64>,
    label: &'a str,
    path_id: Option<i64>,
    metric: Metric,
    color: &'a str,
}

fn insert_node(tx: &rusqlite::Transaction<'_>, insert: NodeInsert<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO hierarchy_node (hierarchy_id, node_id, parent_node_id, label, path_id, metric_role, nar_size, closure_size, unique_bytes, shared_bytes, attributed_bytes, member_count, duplicate_count, color_category) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            insert.hierarchy_id,
            insert.node_id,
            insert.parent_node_id,
            insert.label,
            insert.path_id,
            "store-path",
            insert.metric.nar_size,
            insert.metric.closure_size,
            insert.metric.unique_bytes,
            insert.metric.shared_bytes,
            insert.metric.attributed_bytes,
            insert.metric.member_count as i64,
            insert.metric.duplicate_count as i64,
            insert.color,
        ],
    )?;
    Ok(())
}

#[derive(Clone, Copy)]
struct Metric {
    nar_size: i64,
    closure_size: Option<i64>,
    unique_bytes: i64,
    shared_bytes: i64,
    attributed_bytes: f64,
    member_count: usize,
    duplicate_count: usize,
}

impl Metric {
    fn single(node: &StoreNode, owner_count: usize) -> Self {
        let nar_size = node.nar_size.unwrap_or(0);
        let divisor = owner_count.max(1) as f64;
        Self {
            nar_size,
            closure_size: node.closure_size,
            unique_bytes: if owner_count <= 1 { nar_size } else { 0 },
            shared_bytes: if owner_count > 1 { nar_size } else { 0 },
            attributed_bytes: nar_size as f64 / divisor,
            member_count: 1,
            duplicate_count: usize::from(owner_count > 1),
        }
    }

    fn aggregate(indexes: impl Iterator<Item = usize>, nodes: &[StoreNode], owner_counts: &[usize]) -> Self {
        let mut metric = Self {
            nar_size: 0,
            closure_size: None,
            unique_bytes: 0,
            shared_bytes: 0,
            attributed_bytes: 0.0,
            member_count: 0,
            duplicate_count: 0,
        };

        for index in indexes {
            let child = Self::single(&nodes[index], owner_counts[index]);
            metric.nar_size += child.nar_size;
            metric.unique_bytes += child.unique_bytes;
            metric.shared_bytes += child.shared_bytes;
            metric.attributed_bytes += child.attributed_bytes;
            metric.member_count += 1;
            metric.duplicate_count += child.duplicate_count;
        }

        metric
    }
}

fn owner_counts(node_count: usize, owner_sets: &[BTreeSet<usize>]) -> Vec<usize> {
    let mut counts = vec![0usize; node_count];
    for owner_set in owner_sets {
        for index in owner_set {
            counts[*index] += 1;
        }
    }
    counts
}
