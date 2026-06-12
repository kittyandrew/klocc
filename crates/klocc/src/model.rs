use crate::graph::RuntimeGraph;

macro_rules! string_domain {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

string_domain!(DeriverStatus);
string_domain!(SourceKind);
string_domain!(SourceConfidence);
string_domain!(RealizationStatus);
string_domain!(SourceRelationship);
string_domain!(DependencyKind);

#[derive(Clone, Debug)]
pub struct StoreNode {
    pub path: String,
    pub store_hash: String,
    pub name: String,
    pub nar_size: Option<i64>,
    pub closure_size: Option<i64>,
    pub deriver_status: DeriverStatus,
    pub deriver_path: Option<String>,
    pub references_queried: bool,
    pub references: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CommandRun {
    pub command: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub stderr_excerpt: String,
}

#[derive(Debug)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug)]
pub struct OwnershipRow {
    pub immediate_parent_count: usize,
    pub top_owner_count: usize,
    pub unique_bytes: i64,
    pub shared_bytes: i64,
    pub ownership_weight_json: String,
}

#[derive(Debug)]
pub struct SourceUnit {
    pub name: String,
    pub version: Option<String>,
    pub ecosystem: String,
    pub source_store_path: Option<String>,
    pub origin_url: Option<String>,
    pub origin_rev: Option<String>,
    pub source_kind: SourceKind,
    pub confidence: SourceConfidence,
    pub realization_status: RealizationStatus,
    pub links: Vec<SourceLink>,
    pub loc: Option<SourceLoc>,
}

#[derive(Debug)]
pub struct SourceLink {
    pub package_path_index: usize,
    pub relationship: SourceRelationship,
}

#[derive(Debug)]
pub struct SourceDependency {
    pub from_source_index: usize,
    pub to_source_index: usize,
    pub dependency_kind: DependencyKind,
    pub dependency_spec: Option<String>,
}

#[derive(Debug)]
pub struct SourceRollup {
    pub source_index: usize,
    pub own_code_loc: i64,
    pub transitive_code_loc: i64,
    pub total_code_loc: i64,
    pub unique_transitive_code_loc: i64,
    pub shared_transitive_code_loc: i64,
    pub reachable_source_count: usize,
    pub unique_reachable_source_count: usize,
    pub shared_reachable_source_count: usize,
    pub runtime_linked: bool,
    pub build_time_only: bool,
}

#[derive(Debug)]
pub struct DerivationNode {
    pub drv_path: String,
    pub name: Option<String>,
    pub system: Option<String>,
    pub builder: Option<String>,
    pub is_fixed_output: bool,
    pub raw_json: String,
    pub outputs: Vec<DerivationOutput>,
    pub input_derivations: Vec<DerivationInput>,
    pub input_sources: Vec<String>,
    pub source_links: Vec<DerivationSourceLink>,
}

#[derive(Clone, Debug)]
pub struct DerivationOutput {
    pub output_name: String,
    pub output_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DerivationInput {
    pub input_drv_path: String,
    pub output_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DerivationSourceLink {
    pub source_index: usize,
    pub relationship: SourceRelationship,
}

#[derive(Debug)]
pub struct SourceGraph {
    pub units: Vec<SourceUnit>,
    pub dependencies: Vec<SourceDependency>,
    pub rollups: Vec<SourceRollup>,
    pub derivations: Vec<DerivationNode>,
    pub stats: SourceScanStats,
}

#[derive(Default, Debug)]
pub struct SourceScanStats {
    pub loc_memory_cache_hits: i64,
    pub loc_persistent_cache_hits: i64,
    pub loc_cache_misses: i64,
    pub loc_cache_stores: i64,
}

#[derive(Clone, Debug)]
pub struct SourceLoc {
    pub policy_hash: String,
    pub counter: String,
    pub loc_total: i64,
    pub loc_code: i64,
    pub loc_comments: i64,
    pub loc_blank: i64,
    pub languages: Vec<SourceLanguageLoc>,
}

#[derive(Clone, Debug)]
pub struct SourceLanguageLoc {
    pub language: String,
    pub files: i64,
    pub loc_total: i64,
    pub loc_code: i64,
    pub loc_comments: i64,
    pub loc_blank: i64,
}

#[derive(Clone, Debug)]
pub struct Category {
    pub name: String,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug)]
pub struct ScanData {
    pub root_input: String,
    pub root_store_path: String,
    pub nix_version: String,
    pub commands: Vec<CommandRun>,
    pub nodes: Vec<StoreNode>,
    pub edges: Vec<(usize, usize)>,
    pub graph: RuntimeGraph,
    pub ownership: Vec<OwnershipRow>,
    pub categories: Vec<Category>,
    pub sources: Vec<SourceUnit>,
    pub source_dependencies: Vec<SourceDependency>,
    pub source_rollups: Vec<SourceRollup>,
    pub derivations: Vec<DerivationNode>,
    pub health_metrics: Vec<ScanHealthMetric>,
    pub root_index: usize,
}

#[derive(Debug)]
pub struct ScanHealthMetric {
    pub name: String,
    pub value: i64,
}
