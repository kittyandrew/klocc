#!/usr/bin/env bash
set -euo pipefail

name=${1:?usage: check-real-project.sh NAME TARGET OUT_DIR}
target=${2:?usage: check-real-project.sh NAME TARGET OUT_DIR}
out_dir=${3:?usage: check-real-project.sh NAME TARGET OUT_DIR}

klocc_bin=${KLOCC_BIN:-klocc}
sqlite_bin=${SQLITE_BIN:-sqlite3}
timeout_duration=${KLOCC_REAL_CHECK_TIMEOUT:-1800}

mkdir -p "$out_dir"
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT
artifact="$work_dir/$name.sqlite"
export HOME="$work_dir/home"
export XDG_CACHE_HOME="$work_dir/cache"
mkdir -p "$HOME" "$XDG_CACHE_HOME"

printf 'real-project-check[%s]: target %s\n' "$name" "$target" | tee "$out_dir/summary.txt"
if ! timeout "$timeout_duration" "$klocc_bin" scan "$target" --out "$artifact" >"$out_dir/scan.log" 2>&1; then
  printf 'real-project-check[%s]: scan failed\n' "$name" >&2
  cat "$out_dir/scan.log" >&2
  exit 1
fi
if ! "$klocc_bin" check "$artifact" >"$out_dir/check.log" 2>&1; then
  printf 'real-project-check[%s]: artifact check failed\n' "$name" >&2
  cat "$out_dir/check.log" >&2
  exit 1
fi

query() {
  "$sqlite_bin" "$artifact" "$1"
}

metric() {
  query "SELECT value FROM scan_health WHERE metric = '$1';"
}

assert_ge() {
  local actual=$1
  local expected=$2
  local label=$3
  if ((actual < expected)); then
    printf 'real-project-check[%s]: %s expected >= %s, got %s\n' "$name" "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_le() {
  local actual=$1
  local expected=$2
  local label=$3
  if ((actual > expected)); then
    printf 'real-project-check[%s]: %s expected <= %s, got %s\n' "$name" "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_zero_query() {
  local sql=$1
  local label=$2
  local count
  count=$(query "$sql")
  if ((count != 0)); then
    printf 'real-project-check[%s]: %s found %s violations\n' "$name" "$label" "$count" >&2
    exit 1
  fi
}

runtime_paths=$(metric runtime_paths)
source_units=$(metric source_units)
runtime_linked_source_units=$(metric runtime_linked_source_units)
build_time_only_source_units=$(metric build_time_only_source_units)
generated_outputs=$(metric generated_derivation_outputs)
unknown_derivation_sources=$(metric unknown_derivation_sources)
unknown_runtime_derivers=$(metric unknown_runtime_derivers)
source_units_with_loc=$(metric source_units_with_loc)
source_dependency_edges=$(metric source_dependency_edges)
derivations=$(metric derivations)
derivation_source_unit_links=$(metric derivation_source_unit_links)
total_code_loc=$(query "SELECT COALESCE(SUM(loc_code), 0) FROM source_loc;")
distinct_source_kinds=$(query "SELECT COUNT(DISTINCT source_kind) FROM source_unit;")
distinct_ecosystems=$(query "SELECT COUNT(DISTINCT ecosystem) FROM source_unit;")
distinct_realization_statuses=$(query "SELECT COUNT(DISTINCT realization_status) FROM source_unit;")

assert_ge "$runtime_paths" "${KLOCC_MIN_RUNTIME_PATHS:-1}" runtime_paths
assert_ge "$source_units" "${KLOCC_MIN_SOURCE_UNITS:-1}" source_units
assert_ge "$runtime_linked_source_units" "${KLOCC_MIN_RUNTIME_LINKED_SOURCE_UNITS:-1}" runtime_linked_source_units
assert_ge "$build_time_only_source_units" "${KLOCC_MIN_BUILD_TIME_ONLY_SOURCE_UNITS:-0}" build_time_only_source_units
assert_ge "$generated_outputs" "${KLOCC_MIN_GENERATED_OUTPUTS:-0}" generated_derivation_outputs
assert_ge "$source_units_with_loc" "${KLOCC_MIN_SOURCE_UNITS_WITH_LOC:-0}" source_units_with_loc
assert_ge "$source_dependency_edges" "${KLOCC_MIN_SOURCE_DEPENDENCY_EDGES:-0}" source_dependency_edges
assert_ge "$derivations" "${KLOCC_MIN_DERIVATIONS:-0}" derivations
assert_ge "$derivation_source_unit_links" "${KLOCC_MIN_DERIVATION_SOURCE_UNIT_LINKS:-0}" derivation_source_unit_links
assert_ge "$total_code_loc" "${KLOCC_MIN_TOTAL_CODE_LOC:-0}" total_code_loc
assert_ge "$distinct_source_kinds" "${KLOCC_MIN_DISTINCT_SOURCE_KINDS:-0}" distinct_source_kinds
assert_ge "$distinct_ecosystems" "${KLOCC_MIN_DISTINCT_ECOSYSTEMS:-0}" distinct_ecosystems
assert_ge "$distinct_realization_statuses" "${KLOCC_MIN_DISTINCT_REALIZATION_STATUSES:-0}" distinct_realization_statuses

if [[ -n "${KLOCC_MAX_RUNTIME_PATHS:-}" ]]; then
  assert_le "$runtime_paths" "$KLOCC_MAX_RUNTIME_PATHS" runtime_paths
fi
if [[ -n "${KLOCC_MAX_UNKNOWN_DERIVATION_SOURCES:-}" ]]; then
  assert_le "$unknown_derivation_sources" "$KLOCC_MAX_UNKNOWN_DERIVATION_SOURCES" unknown_derivation_sources
fi
if [[ -n "${KLOCC_MAX_UNKNOWN_RUNTIME_DERIVERS:-}" ]]; then
  assert_le "$unknown_runtime_derivers" "$KLOCC_MAX_UNKNOWN_RUNTIME_DERIVERS" unknown_runtime_derivers
fi
if [[ -n "${KLOCC_MIN_BUILD_TO_RUNTIME_SOURCE_RATIO:-}" ]]; then
  required_build_time_only=$((runtime_linked_source_units * KLOCC_MIN_BUILD_TO_RUNTIME_SOURCE_RATIO))
  assert_ge "$build_time_only_source_units" "$required_build_time_only" build_time_only_source_units_ratio
fi

for kind in ${KLOCC_EXPECT_SOURCE_KINDS:-}; do
  count=$(query "SELECT COUNT(*) FROM source_unit WHERE source_kind = '$kind';")
  assert_ge "$count" 1 "source_kind:$kind"
done

for status in ${KLOCC_EXPECT_REALIZATION_STATUSES:-}; do
  count=$(query "SELECT COUNT(*) FROM source_unit WHERE realization_status = '$status';")
  assert_ge "$count" 1 "realization_status:$status"
done

for view_name in source-kind ecosystem layer; do
  leaf_count=$(query "SELECT COUNT(*) FROM source_treemap_node WHERE view_name = '$view_name' AND source_id IS NOT NULL;")
  if ((leaf_count != source_units)); then
    printf 'real-project-check[%s]: treemap view %s has %s leaves; expected %s\n' "$name" "$view_name" "$leaf_count" "$source_units" >&2
    exit 1
  fi
done

assert_zero_query \
  "SELECT COUNT(*) FROM source_rollup WHERE own_code_loc < 0 OR transitive_code_loc < 0 OR total_code_loc < 0 OR unique_transitive_code_loc < 0 OR shared_transitive_code_loc < 0;" \
  "negative source rollup metrics"
assert_zero_query \
  "SELECT COUNT(*) FROM source_rollup WHERE total_code_loc != own_code_loc + transitive_code_loc;" \
  "source rollup total mismatch"
assert_zero_query \
  "SELECT COUNT(*) FROM source_rollup WHERE transitive_code_loc != unique_transitive_code_loc + shared_transitive_code_loc;" \
  "source rollup transitive mismatch"
assert_zero_query \
  "SELECT COUNT(*) FROM source_rollup WHERE reachable_source_count != unique_reachable_source_count + shared_reachable_source_count;" \
  "source rollup reachable mismatch"
assert_zero_query \
  "SELECT COUNT(*) FROM source_rollup WHERE runtime_linked NOT IN (0, 1) OR build_time_only NOT IN (0, 1) OR runtime_linked + build_time_only != 1;" \
  "source rollup layer flags"
assert_zero_query \
  "SELECT COUNT(*) FROM source_treemap_node node JOIN source_rollup rollup USING (source_id) WHERE node.source_id IS NOT NULL AND (node.own_code_loc != rollup.own_code_loc OR node.total_code_loc != rollup.total_code_loc OR node.unique_transitive_code_loc != rollup.unique_transitive_code_loc OR node.shared_transitive_code_loc != rollup.shared_transitive_code_loc OR node.reachable_source_count != rollup.reachable_source_count OR node.runtime_linked != rollup.runtime_linked OR node.build_time_only != rollup.build_time_only);" \
  "treemap leaf rollup drift"

store_path_count=$(query "SELECT COUNT(*) FROM store_path;")
source_unit_count=$(query "SELECT COUNT(*) FROM source_unit;")
derivation_count=$(query "SELECT COUNT(*) FROM derivation;")
if ((store_path_count != runtime_paths)); then
  printf 'real-project-check[%s]: runtime_paths metric %s does not match store_path rows %s\n' "$name" "$runtime_paths" "$store_path_count" >&2
  exit 1
fi
if ((source_unit_count != source_units)); then
  printf 'real-project-check[%s]: source_units metric %s does not match source_unit rows %s\n' "$name" "$source_units" "$source_unit_count" >&2
  exit 1
fi
if ((derivation_count != $(metric derivations) )); then
  printf 'real-project-check[%s]: derivations metric does not match derivation rows\n' "$name" >&2
  exit 1
fi

{
  printf 'real-project-check[%s]: ok\n' "$name"
  printf '  runtime_paths=%s\n' "$runtime_paths"
  printf '  source_units=%s\n' "$source_units"
  printf '  source_units_with_loc=%s\n' "$source_units_with_loc"
  printf '  runtime_linked_source_units=%s\n' "$runtime_linked_source_units"
  printf '  build_time_only_source_units=%s\n' "$build_time_only_source_units"
  printf '  source_dependency_edges=%s\n' "$source_dependency_edges"
  printf '  derivations=%s\n' "$derivations"
  printf '  derivation_source_unit_links=%s\n' "$derivation_source_unit_links"
  printf '  total_code_loc=%s\n' "$total_code_loc"
  printf '  distinct_source_kinds=%s\n' "$distinct_source_kinds"
  printf '  distinct_ecosystems=%s\n' "$distinct_ecosystems"
  printf '  distinct_realization_statuses=%s\n' "$distinct_realization_statuses"
  printf '  generated_derivation_outputs=%s\n' "$generated_outputs"
  printf '  unknown_derivation_sources=%s\n' "$unknown_derivation_sources"
  printf '  unknown_runtime_derivers=%s\n' "$unknown_runtime_derivers"
  printf '  top_source_kinds=%s\n' "$(query "SELECT group_concat(source_kind || ':' || count, ', ') FROM (SELECT source_kind, COUNT(*) AS count FROM source_unit GROUP BY source_kind ORDER BY count DESC LIMIT 8);")"
  printf '  top_ecosystems=%s\n' "$(query "SELECT group_concat(ecosystem || ':' || count, ', ') FROM (SELECT ecosystem, COUNT(*) AS count FROM source_unit GROUP BY ecosystem ORDER BY count DESC LIMIT 8);")"
} | tee -a "$out_dir/summary.txt"

if [[ "${KLOCC_REAL_CHECK_KEEP_ARTIFACT:-0}" == 1 ]]; then
  cp "$artifact" "$out_dir/scan.sqlite"
fi
