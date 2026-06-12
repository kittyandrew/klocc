use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::model::{DerivationInput, DerivationOutput};

use super::candidates::{
    SourceCandidate, extract_store_paths, relationship_for_env_key, source_candidate_path_from_reference,
    source_env_key, source_kind_for_env_key, source_like_name, source_like_path, store_path_from_json_path,
};

#[derive(Clone, Debug)]
pub(super) struct DerivationInfo {
    pub(super) drv_path: String,
    pub(super) name: Option<String>,
    pub(super) system: Option<String>,
    pub(super) builder: Option<String>,
    pub(super) is_fixed_output: bool,
    pub(super) raw_json: String,
    pub(super) outputs: Vec<DerivationOutput>,
    pub(super) input_drvs: Vec<DerivationInput>,
    pub(super) input_srcs: Vec<SourceCandidate>,
    pub(super) source_candidates: Vec<SourceCandidate>,
}

pub(super) fn parse_derivation_show(value: &Value) -> Result<BTreeMap<String, DerivationInfo>> {
    if let Some(derivations) = value.get("derivations").and_then(Value::as_object) {
        return derivations
            .iter()
            .map(|(drv_name, drv_value)| {
                let drv_path = store_path_from_json_path(drv_name);
                parse_derivation_info(&drv_path, drv_value).map(|info| (drv_path, info))
            })
            .collect();
    }

    value
        .as_object()
        .context("nix derivation show returned unexpected JSON shape")?
        .iter()
        .filter(|(_, drv_value)| drv_value.is_object())
        .map(|(drv_name, drv_value)| {
            let drv_path = store_path_from_json_path(drv_name);
            parse_derivation_info(&drv_path, drv_value).map(|info| (drv_path, info))
        })
        .collect()
}

fn parse_derivation_info(drv_path: &str, value: &Value) -> Result<DerivationInfo> {
    let mut candidates = BTreeMap::<String, SourceCandidate>::new();
    let mut input_drvs = BTreeMap::<String, Vec<String>>::new();
    let mut input_srcs = BTreeMap::<String, SourceCandidate>::new();
    let drv_name = value.get("name").and_then(Value::as_str).unwrap_or(drv_path);
    let name = value.get("name").and_then(Value::as_str).map(ToOwned::to_owned);
    let system = value.get("system").and_then(Value::as_str).map(ToOwned::to_owned);
    let builder = value.get("builder").and_then(Value::as_str).map(ToOwned::to_owned);
    let fixed_output = has_fixed_output(value);

    if let Some(inputs) = value.get("inputs") {
        if let Some(drvs) = inputs.get("drvs").and_then(Value::as_object) {
            for (key, input_value) in drvs {
                input_drvs.insert(store_path_from_json_path(key), input_output_names(input_value));
            }
        }
        if let Some(srcs) = inputs.get("srcs").and_then(Value::as_array) {
            for src in srcs.iter().filter_map(Value::as_str) {
                insert_source_candidate(
                    &mut input_srcs,
                    store_path_from_json_path(src),
                    "nix-input-src".to_string(),
                    "medium".to_string(),
                    "nix-source-input".to_string(),
                    false,
                );
            }
        }
    }
    if let Some(drvs) = value.get("inputDrvs").and_then(Value::as_object) {
        for (key, input_value) in drvs {
            input_drvs.insert(store_path_from_json_path(key), input_output_names(input_value));
        }
    }
    if let Some(srcs) = value.get("inputSrcs").and_then(Value::as_array) {
        for src in srcs.iter().filter_map(Value::as_str) {
            insert_source_candidate(
                &mut input_srcs,
                store_path_from_json_path(src),
                "nix-input-src".to_string(),
                "medium".to_string(),
                "nix-source-input".to_string(),
                false,
            );
        }
    }

    if let Some(args) = value.get("args").and_then(Value::as_array) {
        for arg in args.iter().filter_map(Value::as_str) {
            for path in extract_store_paths(arg) {
                if let Some(source_path) = source_candidate_path_from_reference(&path) {
                    insert_source_candidate(
                        &mut input_srcs,
                        source_path,
                        "derivation-arg-source".to_string(),
                        "medium".to_string(),
                        "nix-source-input".to_string(),
                        true,
                    );
                }
            }
        }
    }

    let outputs = derivation_outputs(value);
    if let Some(output_values) = value.get("outputs").and_then(Value::as_object) {
        for output in output_values.values() {
            if let Some(path) = output.get("path").and_then(Value::as_str) {
                maybe_insert_candidate(
                    &mut candidates,
                    store_path_from_json_path(path),
                    if fixed_output {
                        "fixed-output-source"
                    } else {
                        "source-like-derivation-output"
                    }
                    .to_string(),
                    if fixed_output { "high" } else { "medium" }.to_string(),
                    "package-source".to_string(),
                );
            }
        }
    }

    if let Some(env) = value.get("env").and_then(Value::as_object) {
        for (key, value) in env {
            let Some(text) = value.as_str() else { continue };
            if source_env_key(key) {
                for path in extract_store_paths(text) {
                    candidates.entry(path.clone()).or_insert(SourceCandidate {
                        path,
                        source_kind: source_kind_for_env_key(key),
                        confidence: "high".to_string(),
                        relationship: relationship_for_env_key(key),
                    });
                }
            }
        }
        if fixed_output && let Some(path) = env.get("out").and_then(Value::as_str) {
            let source_path = store_path_from_json_path(path);
            let source_kind = if source_like_path(&source_path) {
                "fixed-output-source"
            } else {
                "fixed-output-fetch"
            };
            insert_source_candidate(
                &mut candidates,
                source_path,
                source_kind.to_string(),
                "high".to_string(),
                "package-source".to_string(),
                false,
            );
        }
    }

    if source_like_name(drv_name)
        && let Some(outputs) = value.get("outputs").and_then(Value::as_object)
    {
        for output in outputs.values() {
            if let Some(path) = output.get("path").and_then(Value::as_str) {
                maybe_insert_candidate(
                    &mut candidates,
                    store_path_from_json_path(path),
                    "source-like-derivation-output".to_string(),
                    "medium".to_string(),
                    "package-source".to_string(),
                );
            }
        }
    }

    Ok(DerivationInfo {
        drv_path: drv_path.to_string(),
        name,
        system,
        builder,
        is_fixed_output: fixed_output,
        raw_json: value.to_string(),
        outputs,
        input_drvs: input_drvs
            .into_iter()
            .map(|(input_drv_path, output_names)| DerivationInput {
                input_drv_path,
                output_names,
            })
            .collect(),
        input_srcs: input_srcs.into_values().collect(),
        source_candidates: candidates.into_values().collect(),
    })
}

fn input_output_names(value: &Value) -> Vec<String> {
    let mut names = if let Some(outputs) = value.as_array() {
        outputs
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect()
    } else if let Some(outputs) = value.get("outputs").and_then(Value::as_array) {
        outputs
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    names.sort();
    names.dedup();
    names
}

fn derivation_outputs(value: &Value) -> Vec<DerivationOutput> {
    let Some(outputs) = value.get("outputs").and_then(Value::as_object) else {
        return Vec::new();
    };
    outputs
        .iter()
        .map(|(output_name, output)| DerivationOutput {
            output_name: output_name.clone(),
            output_path: output
                .get("path")
                .and_then(Value::as_str)
                .map(store_path_from_json_path),
        })
        .collect()
}

fn maybe_insert_candidate(
    candidates: &mut BTreeMap<String, SourceCandidate>,
    path: String,
    source_kind: String,
    confidence: String,
    relationship: String,
) {
    if !source_like_path(&path) {
        return;
    }
    candidates.entry(path.clone()).or_insert(SourceCandidate {
        path,
        source_kind,
        confidence,
        relationship,
    });
}

fn insert_source_candidate(
    candidates: &mut BTreeMap<String, SourceCandidate>,
    path: String,
    source_kind: String,
    confidence: String,
    relationship: String,
    require_source_like_name: bool,
) {
    if require_source_like_name && !source_like_path(&path) {
        return;
    }
    candidates.entry(path.clone()).or_insert(SourceCandidate {
        path,
        source_kind,
        confidence,
        relationship,
    });
}

fn has_fixed_output(value: &Value) -> bool {
    if let Some(env) = value.get("env").and_then(Value::as_object)
        && (env.contains_key("outputHash") || env.contains_key("outputHashAlgo") || env.contains_key("outputHashMode"))
    {
        return true;
    }

    value
        .get("outputs")
        .and_then(Value::as_object)
        .is_some_and(|outputs| outputs.values().any(|output| output.get("hash").is_some()))
}
