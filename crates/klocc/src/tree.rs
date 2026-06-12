use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

#[derive(Debug)]
struct SourceNode {
    name: String,
    version: Option<String>,
    ecosystem: String,
    source_kind: String,
    loc_code: Option<i64>,
    runtime_linked: bool,
    build_time_only: bool,
}

pub fn run(artifact: &Path, max_depth: usize, source_filter: &str) -> Result<()> {
    let conn = Connection::open(artifact).with_context(|| format!("failed to open {}", artifact.display()))?;
    print_runtime_tree(&conn)?;
    println!();
    print_source_tree(&conn, max_depth, source_filter)?;
    Ok(())
}

fn print_runtime_tree(conn: &Connection) -> Result<()> {
    println!("runtime output graph:");
    let root_id: i64 = conn.query_row(
        "SELECT sp.path_id FROM scan s JOIN store_path sp ON sp.path = s.root_store_path LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    let nodes = load_store_nodes(conn)?;
    let edges = load_runtime_edges(conn)?;
    let mut stack = BTreeSet::new();
    print_runtime_node(root_id, "", true, &nodes, &edges, &mut stack);
    Ok(())
}

fn print_source_tree(conn: &Connection, max_depth: usize, source_filter: &str) -> Result<()> {
    println!("source dependency graph ({source_filter}):");
    let root_id: i64 = conn.query_row(
        "SELECT su.source_id FROM source_unit su JOIN package_source ps USING(source_id) WHERE ps.relationship = 'package-source' ORDER BY su.source_id LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    let nodes = load_source_nodes(conn)?;
    let edges = load_source_edges(conn, source_filter)?;
    let mut expanded = BTreeSet::new();
    print_source_node(root_id, "", true, 0, max_depth, &nodes, &edges, &mut expanded);
    Ok(())
}

fn load_store_nodes(conn: &Connection) -> Result<BTreeMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT path_id, name FROM store_path ORDER BY path_id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let mut nodes = BTreeMap::new();
    for row in rows {
        let (id, name) = row?;
        nodes.insert(id, name);
    }
    Ok(nodes)
}

fn load_runtime_edges(conn: &Connection) -> Result<BTreeMap<i64, Vec<i64>>> {
    let mut stmt =
        conn.prepare("SELECT from_path_id, to_path_id FROM runtime_edge ORDER BY from_path_id, to_path_id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    let mut edges = BTreeMap::<i64, Vec<i64>>::new();
    for row in rows {
        let (from, to) = row?;
        edges.entry(from).or_default().push(to);
    }
    Ok(edges)
}

fn load_source_nodes(conn: &Connection) -> Result<BTreeMap<i64, SourceNode>> {
    let mut stmt = conn.prepare(
        "SELECT su.source_id, su.name, su.version, su.ecosystem, su.source_kind, sl.loc_code, sr.runtime_linked, sr.build_time_only FROM source_unit su LEFT JOIN source_loc sl USING(source_id) JOIN source_rollup sr USING(source_id) ORDER BY su.source_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            SourceNode {
                name: row.get(1)?,
                version: row.get(2)?,
                ecosystem: row.get(3)?,
                source_kind: row.get(4)?,
                loc_code: row.get(5)?,
                runtime_linked: row.get(6)?,
                build_time_only: row.get(7)?,
            },
        ))
    })?;
    let mut nodes = BTreeMap::new();
    for row in rows {
        let (id, node) = row?;
        nodes.insert(id, node);
    }
    Ok(nodes)
}

fn load_source_edges(conn: &Connection, source_filter: &str) -> Result<BTreeMap<i64, Vec<(i64, String)>>> {
    let filter = match source_filter {
        "runtime" => "AND child_rollup.runtime_linked = 1",
        "build" => "AND child_rollup.build_time_only = 1",
        "unique" => {
            "AND (SELECT COUNT(*) FROM source_dependency incoming WHERE incoming.to_source_id = sd.to_source_id) <= 1"
        }
        "shared" => {
            "AND (SELECT COUNT(*) FROM source_dependency incoming WHERE incoming.to_source_id = sd.to_source_id) > 1"
        }
        _ => "",
    };
    let query = format!(
        "SELECT sd.from_source_id, sd.to_source_id, COALESCE(sd.dependency_spec, sd.dependency_kind) FROM source_dependency sd JOIN source_rollup child_rollup ON child_rollup.source_id = sd.to_source_id WHERE 1 = 1 {filter} ORDER BY sd.from_source_id, sd.to_source_id"
    );
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    })?;
    let mut edges = BTreeMap::<i64, Vec<(i64, String)>>::new();
    for row in rows {
        let (from, to, spec) = row?;
        edges.entry(from).or_default().push((to, spec));
    }
    Ok(edges)
}

fn print_runtime_node(
    id: i64,
    prefix: &str,
    last: bool,
    nodes: &BTreeMap<i64, String>,
    edges: &BTreeMap<i64, Vec<i64>>,
    stack: &mut BTreeSet<i64>,
) {
    let marker = if stack.is_empty() {
        ""
    } else if last {
        "└── "
    } else {
        "├── "
    };
    let name = nodes.get(&id).map(String::as_str).unwrap_or("<missing>");
    if stack.contains(&id) {
        println!("{prefix}{marker}{name} [cycle]");
        return;
    }
    println!("{prefix}{marker}{name}");
    stack.insert(id);
    let child_prefix = if prefix.is_empty() && stack.len() == 1 {
        String::new()
    } else {
        format!("{}{}", prefix, if last { "    " } else { "│   " })
    };
    let children = edges.get(&id).map(Vec::as_slice).unwrap_or(&[]);
    for (index, child) in children.iter().enumerate() {
        print_runtime_node(*child, &child_prefix, index + 1 == children.len(), nodes, edges, stack);
    }
    stack.remove(&id);
}

#[allow(clippy::too_many_arguments)]
fn print_source_node(
    id: i64,
    prefix: &str,
    last: bool,
    depth: usize,
    max_depth: usize,
    nodes: &BTreeMap<i64, SourceNode>,
    edges: &BTreeMap<i64, Vec<(i64, String)>>,
    expanded: &mut BTreeSet<i64>,
) {
    let marker = if prefix.is_empty() {
        ""
    } else if last {
        "└── "
    } else {
        "├── "
    };
    let Some(node) = nodes.get(&id) else {
        println!("{prefix}{marker}<missing source {id}>");
        return;
    };
    let version = node
        .version
        .as_deref()
        .map_or(String::new(), |version| format!("-{version}"));
    let loc = node.loc_code.map_or(String::new(), |loc| format!(" · {loc} code LOC"));
    let layer = source_layer(node);
    println!(
        "{prefix}{marker}{}{} [{}:{}:{layer}]{}",
        node.name, version, node.ecosystem, node.source_kind, loc
    );
    if depth >= max_depth {
        println!("{prefix}{}└── [max depth reached]", if last { "    " } else { "│   " });
        return;
    }
    if !expanded.insert(id) {
        println!("{prefix}{}└── [already expanded]", if last { "    " } else { "│   " });
        return;
    }
    let child_prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}{}", prefix, if last { "    " } else { "│   " })
    };
    let children = edges.get(&id).map(Vec::as_slice).unwrap_or(&[]);
    for (index, (child, spec)) in children.iter().enumerate() {
        print_source_child(
            *child,
            spec,
            &child_prefix,
            index + 1 == children.len(),
            depth + 1,
            max_depth,
            nodes,
            edges,
            expanded,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn print_source_child(
    id: i64,
    spec: &str,
    prefix: &str,
    last: bool,
    depth: usize,
    max_depth: usize,
    nodes: &BTreeMap<i64, SourceNode>,
    edges: &BTreeMap<i64, Vec<(i64, String)>>,
    expanded: &mut BTreeSet<i64>,
) {
    let marker = if last { "└── " } else { "├── " };
    let Some(node) = nodes.get(&id) else {
        println!("{prefix}{marker}<missing source {id}> via {spec}");
        return;
    };
    let version = node
        .version
        .as_deref()
        .map_or(String::new(), |version| format!("-{version}"));
    let loc = node.loc_code.map_or(String::new(), |loc| format!(" · {loc} code LOC"));
    let layer = source_layer(node);
    println!(
        "{prefix}{marker}{}{} [{}:{}:{layer}] via {}{}",
        node.name, version, node.ecosystem, node.source_kind, spec, loc
    );
    if depth >= max_depth {
        println!("{prefix}{}└── [max depth reached]", if last { "    " } else { "│   " });
        return;
    }
    if !expanded.insert(id) {
        println!("{prefix}{}└── [already expanded]", if last { "    " } else { "│   " });
        return;
    }
    let child_prefix = format!("{}{}", prefix, if last { "    " } else { "│   " });
    let children = edges.get(&id).map(Vec::as_slice).unwrap_or(&[]);
    for (index, (child, child_spec)) in children.iter().enumerate() {
        print_source_child(
            *child,
            child_spec,
            &child_prefix,
            index + 1 == children.len(),
            depth + 1,
            max_depth,
            nodes,
            edges,
            expanded,
        );
    }
}

fn source_layer(node: &SourceNode) -> &'static str {
    if node.runtime_linked {
        "runtime-linked"
    } else if node.build_time_only {
        "build-time-only"
    } else {
        "unclassified"
    }
}
