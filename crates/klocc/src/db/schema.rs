use anyhow::Result;
use rusqlite::Connection;

pub fn create(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE schema_info (
            schema_version INTEGER NOT NULL,
            created_by TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE scan (
            scan_id TEXT PRIMARY KEY,
            root_input TEXT NOT NULL,
            root_store_path TEXT NOT NULL,
            nix_version TEXT,
            policy_json TEXT NOT NULL
        );

        CREATE TABLE scan_command (
            scan_id TEXT NOT NULL REFERENCES scan(scan_id),
            seq INTEGER NOT NULL,
            command TEXT NOT NULL,
            exit_code INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            stderr_excerpt TEXT NOT NULL,
            PRIMARY KEY (scan_id, seq)
        );

        CREATE TABLE scan_health (
            scan_id TEXT NOT NULL REFERENCES scan(scan_id),
            metric TEXT NOT NULL,
            value INTEGER NOT NULL,
            PRIMARY KEY (scan_id, metric)
        );

        CREATE TABLE store_path (
            path_id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            store_hash TEXT NOT NULL,
            name TEXT NOT NULL,
            nar_size INTEGER,
            closure_size INTEGER,
            deriver_status TEXT NOT NULL,
            deriver_path TEXT
        );

        CREATE TABLE runtime_edge (
            from_path_id INTEGER NOT NULL REFERENCES store_path(path_id),
            to_path_id INTEGER NOT NULL REFERENCES store_path(path_id),
            PRIMARY KEY (from_path_id, to_path_id)
        );

        CREATE TABLE runtime_rollup (
            path_id INTEGER PRIMARY KEY REFERENCES store_path(path_id),
            runtime_ref_count INTEGER NOT NULL,
            reverse_ref_count INTEGER NOT NULL,
            added_size INTEGER NOT NULL,
            shared_size INTEGER NOT NULL
        );

        CREATE TABLE ownership_rollup (
            path_id INTEGER PRIMARY KEY REFERENCES store_path(path_id),
            immediate_parent_count INTEGER NOT NULL,
            top_owner_count INTEGER NOT NULL,
            is_unique_to_parent INTEGER NOT NULL,
            unique_bytes INTEGER NOT NULL,
            shared_bytes INTEGER NOT NULL,
            ownership_weight_json TEXT NOT NULL
        );

        CREATE TABLE path_category (
            path_id INTEGER PRIMARY KEY REFERENCES store_path(path_id),
            category TEXT NOT NULL,
            confidence REAL NOT NULL,
            reason TEXT NOT NULL
        );

        CREATE TABLE hierarchy (
            hierarchy_id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL,
            metric_default TEXT NOT NULL
        );

        CREATE TABLE hierarchy_node (
            hierarchy_id INTEGER NOT NULL REFERENCES hierarchy(hierarchy_id),
            node_id INTEGER NOT NULL,
            parent_node_id INTEGER,
            label TEXT NOT NULL,
            path_id INTEGER REFERENCES store_path(path_id),
            metric_role TEXT NOT NULL,
            nar_size INTEGER NOT NULL,
            closure_size INTEGER,
            unique_bytes INTEGER NOT NULL,
            shared_bytes INTEGER NOT NULL,
            attributed_bytes REAL NOT NULL,
            member_count INTEGER NOT NULL,
            duplicate_count INTEGER NOT NULL,
            color_category TEXT NOT NULL,
            PRIMARY KEY (hierarchy_id, node_id),
            FOREIGN KEY (hierarchy_id, parent_node_id) REFERENCES hierarchy_node(hierarchy_id, node_id)
        );

        CREATE TABLE derivation (
            drv_id INTEGER PRIMARY KEY,
            drv_path TEXT NOT NULL UNIQUE,
            name TEXT,
            system TEXT,
            builder TEXT,
            is_fixed_output INTEGER,
            raw_json TEXT
        );

        CREATE TABLE derivation_input (
            drv_id INTEGER NOT NULL REFERENCES derivation(drv_id),
            input_drv_id INTEGER NOT NULL REFERENCES derivation(drv_id),
            output_names_json TEXT NOT NULL,
            PRIMARY KEY (drv_id, input_drv_id)
        );

        CREATE TABLE derivation_output (
            drv_id INTEGER NOT NULL REFERENCES derivation(drv_id),
            output_name TEXT NOT NULL,
            output_path TEXT,
            PRIMARY KEY (drv_id, output_name)
        );

        CREATE TABLE derivation_source_input (
            drv_id INTEGER NOT NULL REFERENCES derivation(drv_id),
            source_path TEXT NOT NULL,
            PRIMARY KEY (drv_id, source_path)
        );

        CREATE TABLE derivation_source_unit (
            drv_id INTEGER NOT NULL REFERENCES derivation(drv_id),
            source_id INTEGER NOT NULL REFERENCES source_unit(source_id),
            relationship TEXT NOT NULL,
            PRIMARY KEY (drv_id, source_id, relationship)
        );

        CREATE TABLE output_derivation (
            path_id INTEGER NOT NULL REFERENCES store_path(path_id),
            drv_id INTEGER NOT NULL REFERENCES derivation(drv_id),
            output_name TEXT NOT NULL,
            PRIMARY KEY (path_id, drv_id, output_name)
        );

        CREATE TABLE source_unit (
            source_id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT,
            ecosystem TEXT NOT NULL,
            source_store_path TEXT,
            origin_url TEXT,
            origin_rev TEXT,
            source_kind TEXT NOT NULL,
            confidence TEXT NOT NULL,
            realization_status TEXT NOT NULL
        );

        CREATE TABLE source_loc (
            source_id INTEGER NOT NULL REFERENCES source_unit(source_id),
            policy_hash TEXT NOT NULL,
            counter TEXT NOT NULL,
            loc_total INTEGER,
            loc_code INTEGER,
            loc_comments INTEGER,
            loc_blank INTEGER,
            PRIMARY KEY (source_id, policy_hash, counter)
        );

        CREATE TABLE source_language_loc (
            source_id INTEGER NOT NULL REFERENCES source_unit(source_id),
            policy_hash TEXT NOT NULL,
            counter TEXT NOT NULL,
            language TEXT NOT NULL,
            files INTEGER NOT NULL,
            loc_total INTEGER NOT NULL,
            loc_code INTEGER NOT NULL,
            loc_comments INTEGER NOT NULL,
            loc_blank INTEGER NOT NULL,
            PRIMARY KEY (source_id, policy_hash, counter, language)
        );

        CREATE TABLE package_source (
            path_id INTEGER NOT NULL REFERENCES store_path(path_id),
            source_id INTEGER NOT NULL REFERENCES source_unit(source_id),
            relationship TEXT NOT NULL,
            PRIMARY KEY (path_id, source_id, relationship)
        );

        CREATE TABLE source_dependency (
            from_source_id INTEGER NOT NULL REFERENCES source_unit(source_id),
            to_source_id INTEGER NOT NULL REFERENCES source_unit(source_id),
            dependency_kind TEXT NOT NULL,
            dependency_spec TEXT,
            PRIMARY KEY (from_source_id, to_source_id, dependency_kind)
        );

        CREATE TABLE source_rollup (
            source_id INTEGER PRIMARY KEY REFERENCES source_unit(source_id),
            own_code_loc INTEGER NOT NULL,
            transitive_code_loc INTEGER NOT NULL,
            total_code_loc INTEGER NOT NULL,
            unique_transitive_code_loc INTEGER NOT NULL,
            shared_transitive_code_loc INTEGER NOT NULL,
            reachable_source_count INTEGER NOT NULL,
            unique_reachable_source_count INTEGER NOT NULL,
            shared_reachable_source_count INTEGER NOT NULL,
            runtime_linked INTEGER NOT NULL,
            build_time_only INTEGER NOT NULL
        );

        CREATE TABLE source_treemap_node (
            view_name TEXT NOT NULL,
            node_id INTEGER NOT NULL,
            parent_node_id INTEGER,
            source_id INTEGER REFERENCES source_unit(source_id),
            label TEXT NOT NULL,
            group_key TEXT NOT NULL,
            color_key TEXT NOT NULL,
            own_code_loc INTEGER NOT NULL,
            total_code_loc INTEGER NOT NULL,
            unique_transitive_code_loc INTEGER NOT NULL,
            shared_transitive_code_loc INTEGER NOT NULL,
            reachable_source_count INTEGER NOT NULL,
            runtime_linked INTEGER NOT NULL,
            build_time_only INTEGER NOT NULL,
            source_kind TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            realization_status TEXT NOT NULL,
            PRIMARY KEY (view_name, node_id),
            FOREIGN KEY (view_name, parent_node_id) REFERENCES source_treemap_node(view_name, node_id)
        );

        CREATE TABLE why_depends_cache (
            root_path_id INTEGER NOT NULL REFERENCES store_path(path_id),
            target_path_id INTEGER NOT NULL REFERENCES store_path(path_id),
            mode TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            stdout TEXT NOT NULL,
            parsed_json TEXT,
            PRIMARY KEY (root_path_id, target_path_id, mode)
        );

        CREATE INDEX runtime_edge_from_path_id ON runtime_edge(from_path_id);
        CREATE INDEX runtime_edge_to_path_id ON runtime_edge(to_path_id);
        CREATE INDEX ownership_rollup_path_id ON ownership_rollup(path_id);
        CREATE INDEX hierarchy_node_parent ON hierarchy_node(hierarchy_id, parent_node_id);
        CREATE INDEX hierarchy_node_path ON hierarchy_node(hierarchy_id, path_id);
        CREATE INDEX source_dependency_from ON source_dependency(from_source_id);
        CREATE INDEX source_dependency_to ON source_dependency(to_source_id);
        CREATE INDEX source_treemap_node_source_id ON source_treemap_node(source_id);
        CREATE INDEX source_treemap_node_parent ON source_treemap_node(view_name, parent_node_id);
        CREATE INDEX derivation_input_drv_id ON derivation_input(drv_id);
        CREATE INDEX derivation_input_input_drv_id ON derivation_input(input_drv_id);
        CREATE INDEX derivation_output_path ON derivation_output(output_path);
        CREATE INDEX derivation_source_unit_source_id ON derivation_source_unit(source_id);
        ",
    )?;
    Ok(())
}
