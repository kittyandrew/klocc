use std::collections::{HashMap, HashSet};

use crate::model::{DerivationSourceLink, SourceDependency, SourceLink, SourceUnit};

use super::naming::{package_name_from_store_name, unit_key, version_from_store_name};

#[derive(Default)]
pub(super) struct SourceBuilder {
    pub(super) units: Vec<SourceUnit>,
    pub(super) dependencies: Vec<SourceDependency>,
    dependency_keys: HashSet<(usize, usize, String)>,
    unit_by_key: HashMap<String, usize>,
    pub(super) derivation_source_links: HashMap<String, Vec<DerivationSourceLink>>,
}

impl SourceBuilder {
    pub(super) fn add_unit(&mut self, unit: SourceUnit) -> usize {
        let key = unit_key(
            &unit.ecosystem,
            &unit.name,
            unit.version.as_deref(),
            unit.source_store_path.as_deref().or(unit.origin_url.as_deref()),
        );
        if let Some(index) = self.unit_by_key.get(&key) {
            merge_unit(&mut self.units[*index], unit);
            return *index;
        }
        let index = self.units.len();
        self.units.push(unit);
        self.unit_by_key.insert(key, index);
        index
    }

    pub(super) fn add_dependency(&mut self, from: usize, to: usize, kind: &str, dependency_spec: Option<String>) {
        if from == to {
            return;
        }
        if !self.dependency_keys.insert((from, to, kind.to_string())) {
            return;
        }
        self.dependencies.push(SourceDependency {
            from_source_index: from,
            to_source_index: to,
            dependency_kind: kind.into(),
            dependency_spec,
        });
    }

    pub(super) fn add_derivation_source_link(&mut self, drv_path: &str, source_index: usize, relationship: &str) {
        let links = self.derivation_source_links.entry(drv_path.to_string()).or_default();
        if links
            .iter()
            .any(|link| link.source_index == source_index && link.relationship.as_str() == relationship)
        {
            return;
        }
        links.push(DerivationSourceLink {
            source_index,
            relationship: relationship.into(),
        });
    }
}

pub(super) fn add_unknown_derivation_source(builder: &mut SourceBuilder, drv_path: &str) -> usize {
    let store_name = drv_path.rsplit('/').next().unwrap_or(drv_path);
    builder.add_unit(SourceUnit {
        name: package_name_from_store_name(store_name),
        version: version_from_store_name(store_name),
        ecosystem: "nix".to_string(),
        source_store_path: None,
        origin_url: None,
        origin_rev: None,
        source_kind: "unknown-derivation-source".into(),
        confidence: "low".into(),
        realization_status: "no-source-candidate".into(),
        links: Vec::new(),
        loc: None,
    })
}

pub(super) fn add_generated_derivation_source(builder: &mut SourceBuilder, drv_path: &str) -> usize {
    let store_name = drv_path
        .rsplit('/')
        .next()
        .unwrap_or(drv_path)
        .strip_suffix(".drv")
        .unwrap_or(drv_path);
    builder.add_unit(SourceUnit {
        name: package_name_from_store_name(store_name),
        version: version_from_store_name(store_name),
        ecosystem: "nix".to_string(),
        source_store_path: None,
        origin_url: None,
        origin_rev: None,
        source_kind: "generated-derivation-output".into(),
        confidence: "medium".into(),
        realization_status: "generated-from-inputs".into(),
        links: Vec::new(),
        loc: None,
    })
}

fn merge_unit(existing: &mut SourceUnit, incoming: SourceUnit) {
    let incoming_quality = source_quality(&incoming);
    merge_links(existing, incoming.links);
    if existing.loc.is_none() {
        existing.loc = incoming.loc;
    }
    if incoming_quality > source_quality(existing) {
        existing.source_kind = incoming.source_kind;
        existing.confidence = incoming.confidence;
        existing.realization_status = incoming.realization_status;
    }
}

fn merge_links(unit: &mut SourceUnit, links: Vec<SourceLink>) {
    for link in links {
        if !unit.links.iter().any(|existing| {
            existing.package_path_index == link.package_path_index && existing.relationship == link.relationship
        }) {
            unit.links.push(link);
        }
    }
}

fn source_quality(unit: &SourceUnit) -> u8 {
    match (unit.source_kind.as_str(), unit.realization_status.as_str()) {
        (_, "realized" | "available" | "metadata-only") => 4,
        ("generated-derivation-output", "generated-from-inputs") => 3,
        ("unknown-derivation-source", "derivation-unavailable" | "no-source-candidate") => 2,
        ("unknown-source", "unknown-deriver") => 0,
        (_, "missing" | "realize-command-succeeded-but-missing") => 1,
        _ => 1,
    }
}
