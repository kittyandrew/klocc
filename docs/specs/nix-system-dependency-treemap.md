# Nix System Dependency Treemap -- PRD

**Version:** 0.6 (Implementation Draft)
**Date:** May 21, 2026
**Status:** Phase 1 CLI implementation in progress. Native app deferred.

## 1. Project Overview

**Working name:** Nix System Dependency Treemap

**One-line description:** A Rust scanner CLI that uses a selected Nix installable/store path as the
package selection boundary, derives runtime dependency graph data plus source provenance from Nix
metadata, counts discovered source with `tokei`, and writes a normalized SQLite artifact for later
treemap exploration.

**Core value proposition:** Make the system legible without turning the UI into the data pipeline.
The CLI generates, processes, stores, and summarizes the scan; the native app loads the resulting
file and focuses on interactive treemap exploration.

## 2. Problem Statement

Nix makes software reproducible, but the resulting dependency graph is hard to understand at human
scale. A single package closure can already contain hundreds or thousands of store paths; a desktop
or server NixOS system closure can contain many more. Those closures include nested toolchains,
firmware, language ecosystems, generated artifacts, shared libraries, and fixed-output source
fetches. Existing tools answer narrow questions well, but they do not provide one coherent
interactive map of a selected flake installable. The project should separate data generation from
viewing so large scans are reproducible, cacheable, scriptable, and explorable later without
re-running Nix.

The motivating questions are:

- What are all the things that run, or can run, on this NixOS machine?
- What are all the runtime dependencies of a specific flake package, app, or host output?
- Which store paths make up the current system closure?
- Which dependencies are runtime references versus build-time inputs?
- Why does a specific package, library, compiler, firmware blob, or script exist in the closure?
- How much closure size is unique versus shared?
- Where did each source tree come from?
- How many countable source lines are attributable to the selected runtime packages' source inputs?
- Which source is package source versus build-time helper/tool source?
- Where are the unknowns: binary-only artifacts, missing derivers, generated code, vendored deps,
  or source that Nix metadata cannot identify?

## 2.1 Current Implementation Direction

The scanner has two related but distinct responsibilities:

- Runtime closure discovery defines **which realized output paths are in scope** for a selected
  installable. Runtime paths are not treated as source code by default.
- Derivation/source discovery defines **where source code came from** for those scoped runtime
  outputs. LOC is counted from source units discovered through derivers and derivation metadata.

Phase 1 must not conflate runtime binaries with source trees. A runtime path such as `glibc` or a
compiled Rust binary contributes runtime size and graph edges. Its source LOC is counted only if the
scanner can identify a source-like store path through derivation metadata, fixed-output/source-like
outputs, `inputSrcs`, or conventional source env fields such as `src`, `srcs`, or vendor source
paths.

The runtime graph remains useful because it answers "what is actually in this installable's runtime
closure?" The source graph answers "what source was used to build the packages represented by that
runtime closure?" The later native app should make this distinction visible instead of collapsing
both into one tree.

Source relationship labels are required:

- `package-source`: source directly associated with the package/output being inspected, such as
  `env.src`, `env.srcs`, or equivalent source-like fields on the output's own deriver.
- `vendored-source`: language/vendor bundles used to build the package, such as Cargo vendor dirs.
- `recursive-package-source`: source discovered by walking package dependency derivations that are
  themselves runtime package outputs.
- `build-time-source`: source for build tools, hooks, scripts, compilers, setup helpers, or other
  build-time-only inputs. This label is part of the schema/policy vocabulary, but detailed UI and
  rollup treatment can be deferred.
- `unknown-source`: source could not be identified or realized; record the gap honestly.

The scanner has one complete scan path. It should not expose shortcut profiles that silently skip
build-time provenance, source realization, or LOC counting. Runtime-only views are presentation
filters over the complete artifact, not alternate scan modes. If completeness fails, the artifact
must record the missing piece as scan health/provenance data rather than pretending the graph was
smaller.

Build-time-only source classification may be conservative, but it should not silently merge
build-time helper source into package source.

Existing tools fall short because they usually optimize for one axis:

- `nix-tree` is the closest interactive dependency browser, but it is a TUI focused on closure
  browsing and size, not source provenance or a native treemap artifact explorer.
- `nix-du` is strong for disk usage and root attribution, but not for full semantic explanation.
- `nix why-depends` explains one path at a time, not the whole system.
- `nix derivation show -r` exposes build-time JSON, but leaves source and LOC interpretation to
  the caller.
- Static Graphviz tools do not scale to full NixOS closures.

## 3. Target Users

Primary user:

- The repo owner, using the tool locally against KittyOS hosts such as `palanok`, `kodak`,
  `tustan`, `buchach`, and `khotyn`.

Secondary users:

- NixOS users who want to understand closure bloat, hidden build inputs, source availability, and
  package provenance.
- Nixpkgs contributors investigating dependency size, source policy, or binary-only artifacts.
- Security-minded users auditing what code is present in a system and where source is unavailable.

## 4. Core Concepts

### 4.1 Scan Root

The selected Nix installable or store path. A NixOS host is not itself a derivation, but the NixOS
configuration exposes a toplevel system derivation at `config.system.build.toplevel`. Package and
app outputs are also derivations/installables and should use the same scan path.

Examples:

- A live machine root: `/run/current-system`
- A flake host output: `.#nixosConfigurations.palanok.config.system.build.toplevel`
- A flake package output: `.#packages.x86_64-linux.default`
- A flake app package output: `.#apps.x86_64-linux.default` resolved through its package/program
- A remote package output: `github:kittyandrew/waybap#packages.x86_64-linux.default`
- A built result path: `/nix/store/...-nixos-system-palanok-...`

Flake installables are preferred for source and derivation provenance because live store paths can
have `unknown-deriver` when substituted or produced outside the current evaluation context. The MVP
should target package-sized flake outputs first, then scale to full NixOS host toplevel outputs using
the same extraction pipeline.

### 4.2 Runtime Closure Graph

The DAG of realized store output paths reachable from the system root through runtime references.
This is what Nix must keep present for the system output to be valid.

Runtime references are discovered by Nix through store path hash references inside store objects.
They are not the same as semantic application dependencies or NixOS option causality.

### 4.3 Live Process Artifact Graph

The subset of store paths actually mapped or executed by running processes at a point in time. This
is collected from `/proc`, for example:

- `/proc/<pid>/exe` for process executables
- `/proc/<pid>/maps` for loaded shared libraries and mapped files
- systemd unit metadata where available

This graph answers "what is running right now?" It is distinct from the full system closure, which
answers "what can run or is needed by the configured system?"

### 4.4 Build Derivation Graph

The recursive `.drv` graph exposed by `nix derivation show -r`. This is the build-time graph: build
tools, source fetches, fixed-output derivations, patches, language dependency vendors, and package
derivations.

### 4.5 Source Unit

A source tree, tarball, patch set, vendor bundle, generated source directory, or source-like store
path identified from derivation JSON and Nixpkgs conventions.

Source units carry a confidence tag because Nix does not have a universal semantic `source` object.

### 4.6 LOC Measurement

A count of text source lines produced by `tokei` as the default LOC engine. LOC is not a perfect
fact for a Nix closure. It is a measured result under a policy:

- Include or exclude vendored dependencies
- Include or exclude generated/minified code
- Include or exclude tests, docs, examples
- Count patches separately
- Deduplicate by store path or content hash
- Mark binary-only and unknown artifacts explicitly

### 4.7 Treemap Hierarchy

A derived tree used for visualization. Nix dependency data is a DAG, not a tree, so the tool must
support multiple deliberate hierarchy projections rather than pretending there is one canonical
tree.

Examples:

- `system -> runtime layer -> package -> output`
- `system -> live process -> mapped store path`
- `flake input -> package -> output`
- `language -> source origin -> package`
- `license -> package`
- `source confidence -> source kind -> package`
- `closure size bucket -> package`

## 5. Architecture

### 5.1 High-Level Diagram

```text
+-----------------------------+
| NixOS system root            |
| /run/current-system or flake |
+-------------+---------------+
              |
              v
+-----------------------------+
| Scanner/processor CLI        |
| nix path-info                |
| nix-store --query            |
| nix derivation show -r       |
| source/LOC analyzers         |
| stats and summaries          |
+-------------+---------------+
              |
              | normalized SQLite artifact
              v
+-----------------------------+
| Scan database                |
| graph tables                 |
| source tables                |
| metric rollups               |
| hierarchy materializations   |
+-------------+---------------+
              |
              | opened by path
              v
+-----------------------------+
| Native treemap app           |
| minimal UI                   |
| GPUI treemap view            |
| details panel                |
| future graph panes           |
+-----------------------------+
```

### 5.2 Process Model

The product has two components:

```bash
klocc scan <flake-installable-or-store-path> --out scan.sqlite
klocc stats system-scan.sqlite
klocc explain system-scan.sqlite <store-path>
klocc-gui system-scan.sqlite
```

`scan` performs extraction, analysis, rollup computation, and writes the normalized SQLite artifact.
It should be safe by default:

- It does not build arbitrary derivations by default.
- It may evaluate flake installables to obtain derivation JSON.
- It counts only realized source paths in Phase 1.
- It requires an explicit flag before realizing missing source fixed-output derivations.
- It records every command, Nix version, counter version, and policy flag in the scan manifest.

`stats` prints limited terminal output for quick inspection and scripting. It should report totals,
top contributors, unknown counts, and scan health without trying to be an interactive browser.

`explain` performs or reads a lazy explanation for a selected store path. If the explanation is not
already cached, it may run `nix why-depends --precise` and write the result back into the database.

`klocc-gui` is a native desktop application. It takes a SQLite path as input and opens
the file directly. The first version should be intentionally minimal: file load, hierarchy selector,
metric selector, treemap, search, and details panel.

### 5.3 Key Design Decisions

- Use supported Nix CLI outputs first, not Nix store database internals.
- Treat runtime references, live process artifacts, build derivations, and source units as separate
  graph layers.
- Use confidence-tagged source attribution instead of claiming impossible exactness.
- Keep CLI output intentionally limited: generation, processing, stats, totals, and targeted
  explanation commands, not a terminal UI.
- Use a normalized SQLite artifact as the boundary between data processing and exploration.
- Use a native app as the exploration interface. It loads a SQLite file path directly; it does not
  require a local web server.
- Use treemaps for first interactive exploration. Focused graph views are later additions.
- Compute expensive `why-depends --precise` explanations lazily through the CLI/backend and cache
  them in SQLite.
- Store scan results in a durable local database so repeated app exploration does not re-run Nix.

### 5.4 CLI Contract

The CLI is the data product. It should be usable without the app for automation, regression checks,
and quick totals.

```bash
# Generate a package-sized scan artifact.
klocc scan .#packages.x86_64-linux.default --out package.sqlite

# Generate a remote package scan artifact.
klocc scan github:kittyandrew/waybap#packages.x86_64-linux.default --out waybap.sqlite

# Generate a live system scan artifact.
klocc scan /run/current-system --out current.sqlite

# Generate a scan from a KittyOS host output, preserving better derivation provenance.
klocc scan \
  .#nixosConfigurations.palanok.config.system.build.toplevel \
  --out palanok.sqlite

# Print limited totals and top contributors.
klocc stats palanok.sqlite

# Explain one selected path and cache the result in the database.
klocc explain palanok.sqlite /nix/store/...-openssl-...

# Validate artifact schema and scan health.
klocc check palanok.sqlite
```

`stats` should stay compact. Example shape:

```text
scan: palanok.sqlite
root: /nix/store/...-nixos-system-palanok-...
nix: 2.34.7

runtime closure:
  paths: 8421
  edges: 32104
  nar size: 18.4 GiB
  root closure size: 18.4 GiB
  unknown derivers: 2197

top nar size:
  1. 912.3 MiB  /nix/store/...-linux-...
  2. 614.8 MiB  /nix/store/...-firefox-...
  3. 512.1 MiB  /nix/store/...-firmware-...

scan health:
  runtime graph: ok
  derivation graph: partial
  source loc: not collected
```

The CLI should support machine-readable output later (`--json`), but the first priority is a stable
SQLite artifact plus readable terminal summaries.

### 5.5 Native App Contract

The app is an artifact viewer. It should not hide expensive scans behind UI magic in the MVP.

```bash
klocc-gui palanok.sqlite
```

MVP app responsibilities:

- Open one SQLite file.
- Validate schema version.
- Show scan health before rendering.
- Query precomputed hierarchy tables.
- Render one treemap at a time.
- Show details from local tables.
- Optionally invoke `klocc explain` for a selected store path after user action.

MVP app non-responsibilities:

- Running full scans.
- Editing policy flags.
- Managing multiple scan databases.
- Performing source realization.
- Rendering the full raw graph.

## 6. MVP SQLite Schema

The storage format should be normalized enough that the CLI, app, and future diff tools can share
one artifact. SQLite is the interchange format, not just a cache.

### 6.1 Schema Metadata

| Table | Key fields | Purpose |
|-------|------------|---------|
| `schema_info` | `schema_version`, `created_by`, `created_at` | App compatibility and migrations |
| `scan` | `scan_id`, `root_input`, `root_store_path`, `nix_version`, `policy_json` | One row per scan artifact |
| `scan_command` | `scan_id`, `seq`, `command`, `exit_code`, `duration_ms`, `stderr_excerpt` | Reproducibility and debugging |

### 6.2 Runtime Graph Tables

| Table | Key fields | Purpose |
|-------|------------|---------|
| `store_path` | `path_id`, `path`, `store_hash`, `name`, `nar_size`, `closure_size`, `deriver_status`, `deriver_path` | Runtime closure nodes |
| `runtime_edge` | `from_path_id`, `to_path_id` | Store reference edges inside the selected closure |
| `runtime_rollup` | `path_id`, `runtime_ref_count`, `reverse_ref_count`, `added_size`, `shared_size` | Precomputed metrics for sorting and treemaps |
| `ownership_rollup` | `path_id`, `immediate_parent_count`, `top_owner_count`, `is_unique_to_parent`, `unique_bytes`, `shared_bytes`, `ownership_weight_json` | Truthful unique/shared ownership metrics for treemap overlays |
| `path_category` | `path_id`, `category`, `confidence`, `reason` | Heuristic category classification |

### 6.3 Build And Source Tables

These tables can exist in Phase 1 with zero rows or partial rows. Phase 2 populates them.

| Table | Key fields | Purpose |
|-------|------------|---------|
| `derivation` | `drv_id`, `drv_path`, `name`, `system`, `builder`, `is_fixed_output`, `raw_json` | Build-time graph nodes |
| `derivation_input` | `drv_id`, `input_drv_id`, `output_names_json` | Build-time derivation edges |
| `derivation_source_input` | `drv_id`, `source_path` | Literal source inputs from derivation JSON |
| `output_derivation` | `path_id`, `drv_id`, `output_name` | Join from runtime output path to derivation |
| `source_unit` | `source_id`, `source_store_path`, `origin_url`, `origin_rev`, `source_kind`, `confidence`, `realization_status` | Countable or uncountable source origins |
| `source_loc` | `source_id`, `policy_hash`, `counter`, `loc_total`, `loc_code`, `loc_comments`, `loc_blank`, `language_json` | LOC results keyed by policy |
| `package_source` | `path_id`, `source_id`, `relationship` | Links runtime/build outputs to source units |

### 6.4 UI Tables

| Table | Key fields | Purpose |
|-------|------------|---------|
| `hierarchy` | `hierarchy_id`, `name`, `description`, `metric_default` | Available treemap projections |
| `hierarchy_node` | `hierarchy_id`, `node_id`, `parent_node_id`, `label`, `path_id`, `metric_json`, `color_json` | Precomputed treemap tree nodes |
| `why_depends_cache` | `root_path_id`, `target_path_id`, `mode`, `created_at`, `stdout`, `parsed_json` | Cached targeted explanations |

Indexes required for MVP:

- `store_path(path)` unique
- `runtime_edge(from_path_id)`
- `runtime_edge(to_path_id)`
- `ownership_rollup(path_id)`
- `hierarchy_node(hierarchy_id, parent_node_id)`
- `hierarchy_node(hierarchy_id, path_id)`
- `why_depends_cache(root_path_id, target_path_id, mode)` unique

## 7. Existing Tools And What They Provide

| Tool | Best use | Data exposed | Gaps this project fills |
|------|----------|--------------|--------------------------|
| `nix path-info` | Runtime closure node list and sizes | Store paths, NAR size, closure size, JSON, derivation path | No edges, source, LOC, UI |
| `nix-store --query` | Runtime edges and store metadata | References, requisites, referrers, deriver, DOT, GraphML, tree | Raw graph only, no semantic analysis |
| `nix why-depends` | Explain why one path depends on another | Shortest/all paths, precise files causing refs | One query at a time, not aggregate UI |
| `nix derivation show -r` | Build-time graph and source hints | `.drv` JSON, inputs, outputs, env, fixed-output hashes/URLs | Requires inference and policy decisions |
| `nix store ls` | Files inside store paths | Recursive file names and sizes | Artifact view only, no source provenance |
| `nix-tree` | Interactive closure browsing | Closure size, NAR size, added size, search, sort, why-style UX | TUI, no source LOC, no native treemap artifact explorer |
| `nix-du` | Disk/closure size attribution | Root-focused graph, DOT output, size filters | Disk cleanup focus, not source or live process view |
| `nix-visualize` | Static graph images/CSV | Graphviz/matplotlib views | Static and scale-limited |
| `nix-query-tree-viewer` | Foldable tree browsing | GTK view of `nix-store --tree` | Tree projection hides DAG sharing |
| `nix-output-monitor` | Live build progress tree | Build/download events and derivation progress | Not an installed-system explorer |
| `klocc` | LOC counting | Existing repo-owned Rust LOC counter | Needs source discovery from Nix; should become the default LOC engine |
| `scc` | LOC counting fallback/reference | JSON/CSV/SQL/OpenMetrics LOC, generated/minified/duplicate controls | External fallback, not the target default |
| `tokei` | Fast LOC counting fallback/reference | JSON language LOC breakdown | External fallback, not the target default |
| `cloc` | Archive-friendly LOC counting fallback/reference | Mature LOC reports for many formats | External fallback, not the target default |

## 8. Data Model

### 8.1 Scan Manifest

| Field | Type | Notes |
|-------|------|-------|
| `scan_id` | string | Stable ID for this scan |
| `created_at` | timestamp | Scan time |
| `nix_version` | string | From `nix --version` |
| `root_input` | string | User-provided installable or path |
| `root_store_path` | string | Realized system output path |
| `flake_ref` | string nullable | Flake path/ref when available |
| `host_name` | string nullable | NixOS host if detected |
| `policy` | object | LOC/source/build policy flags |
| `commands` | array | Commands run, args, exit status, duration |

### 8.2 Store Path Node

| Field | Type | Notes |
|-------|------|-------|
| `path` | string | Full `/nix/store/...` path |
| `store_hash` | string | Hash prefix |
| `name` | string | Store path name after hash |
| `nar_size` | integer nullable | From `nix path-info --size` or `nix-store --size` |
| `closure_size` | integer nullable | From `nix path-info --closure-size` |
| `added_size` | integer nullable | Non-double-counted size attribution for a selected hierarchy |
| `output_name` | string nullable | `out`, `dev`, `lib`, etc. when known |
| `deriver_path` | string nullable | From Nix if known |
| `deriver_status` | enum | `known`, `unknown`, `not-applicable`, `not-local` |
| `category` | enum | `system`, `kernel`, `firmware`, `library`, `app`, `service`, `toolchain`, `source`, `data`, `unknown` |
| `license_id` | string nullable | From package metadata when available |
| `runtime_ref_count` | integer | Immediate outgoing runtime references |
| `reverse_ref_count` | integer | Incoming references inside selected closure |

### 8.3 Runtime Edge

| Field | Type | Notes |
|-------|------|-------|
| `from_path` | string | Referrer |
| `to_path` | string | Referenced path |
| `edge_kind` | enum | `runtime-reference` |
| `evidence_status` | enum | `known-edge`, `precise-files-loaded`, `precise-files-unavailable` |
| `evidence_files` | array nullable | Populated lazily from `nix why-depends --precise` |

### 8.4 Derivation Node

| Field | Type | Notes |
|-------|------|-------|
| `drv_path` | string | `.drv` path |
| `name` | string | Derivation name |
| `system` | string | Build platform |
| `builder` | string | Builder executable |
| `args` | array | Builder args |
| `env` | object | Raw env from derivation JSON |
| `outputs` | object | Output name to output metadata |
| `input_drvs` | array | Build-time derivation inputs |
| `input_srcs` | array | Literal source inputs |
| `is_fixed_output` | boolean | Any output has fixed content hash |
| `source_confidence` | enum nullable | Best source inference confidence |

### 8.5 Source Unit

| Field | Type | Notes |
|-------|------|-------|
| `source_id` | string | Stable hash over source identity |
| `source_store_path` | string nullable | Realized source path if available |
| `origin_url` | string nullable | URL or repository when inferred |
| `origin_rev` | string nullable | Commit/revision/tag when inferred |
| `content_hash` | string nullable | NAR/output hash when known |
| `source_kind` | enum | `main-src`, `patch`, `vendored-deps`, `generated`, `binary-source`, `local-path`, `unknown` |
| `realization_status` | enum | `realized`, `missing`, `fetchable`, `unavailable`, `not-attempted` |
| `confidence` | enum | `exact-store-count`, `fetched-source-count`, `heuristic-source`, `unavailable`, `unknown` |
| `loc_total` | integer nullable | Total LOC under active policy |
| `loc_code` | integer nullable | Code lines |
| `loc_comments` | integer nullable | Comment lines |
| `loc_blank` | integer nullable | Blank lines |
| `language_breakdown` | object nullable | Counter-specific language totals |
| `counter` | string nullable | `klocc` plus version/policy hash; other counters only as optional future fallbacks |

### 8.6 Live Process Node

| Field | Type | Notes |
|-------|------|-------|
| `pid` | integer | Process ID at scan time |
| `comm` | string | Process name |
| `exe_path` | string nullable | `/proc/<pid>/exe` target |
| `unit` | string nullable | systemd unit if mapped |
| `store_paths` | array | Store paths observed in exe/maps |
| `scan_visibility` | enum | `full`, `partial-permission`, `exited`, `error` |

## 9. Feature Details

### 9.1 Graph Extraction

The extractor must support package and system roots through the same path:

```bash
# Package or app output, ideal for MVP and PoC scans
klocc scan .#packages.x86_64-linux.default --out package.sqlite

# Remote package output, useful for testing a project such as waybap
klocc scan github:kittyandrew/waybap#packages.x86_64-linux.default --out waybap.sqlite

# Evaluated host closure with derivation provenance
klocc scan .#nixosConfigurations.palanok.config.system.build.toplevel --out state.sqlite

# Live system closure, weaker provenance if derivers are unknown
klocc scan /run/current-system --out current.sqlite
```

Runtime extraction:

- Run `nix path-info -r --json --size --closure-size <root>`.
- For every closure path, run `nix-store -q --references <path>` and keep only edges whose target
  is inside the selected closure.
- Compute reverse adjacency from the edge list.
- Record self size, closure size, direct reference count, reverse reference count, and path name.

Build extraction:

- Prefer `nix derivation show -r <flake-host-installable>`.
- If only a store path is provided, try `nix-store -q --deriver <path>` and record
  `unknown-deriver` honestly.
- Parse raw derivation JSON and preserve it enough that later versions can improve source
  inference without rescanning.

### 9.2 Source Inference And LOC Counting

The analyzer classifies source units from derivation JSON using ordered rules.

High-confidence rules:

- Fixed-output derivation output with a content hash and fetcher-like env metadata.
- `env.src` pointing to a realized store path.
- `inputs.srcs` entries that are realized source files, patches, or source directories.
- Flake input metadata when the scanner can associate it with a source store path.

Medium-confidence rules:

- Store path names matching source-like patterns: `source`, `*-src`, `*-source`, `*-tarball`,
  `*-vendor`, `*-cargo-deps`, `*-npm-deps`.
- Fixed-output derivations with non-standard env metadata.

Low-confidence rules:

- Generated source directories.
- Bundled dependency trees where upstream identity is not preserved.
- Language package manager caches whose ownership is ambiguous.

LOC counting policy flags:

| Flag | Default | Meaning |
|------|---------|---------|
| `--include-vendored` | false | Count vendored/language dependency bundles into source LOC |
| `--include-generated` | false | Count generated/minified source when counter can identify it |
| `--include-tests` | true | Count tests in upstream source trees |
| `--include-docs` | false | Count documentation source where language counters classify it |
| `--realize-missing-sources` | false | Realize source fixed-output derivations before counting |
| `--counter` | `klocc` | LOC counter command/library |

The UI must never show a single LOC number without exposing its policy and uncertainty. Rollups
should show:

- Counted source LOC
- Vendored dependency LOC
- Patch LOC
- Generated LOC if included
- Unknown source units
- Binary/source-unavailable units

### 9.3 Size Attribution

Closure size is shared in a DAG. Naively summing each node's closure size overcounts massively.

The analyzer should provide multiple metrics:

- `nar_size`: the path's own serialized size.
- `closure_size`: the full transitive closure size under this path.
- `added_size`: size uniquely added when the path is included in a selected traversal or hierarchy,
  inspired by `nix-tree` and `nix-du`.
- `shared_size`: size reachable from multiple selected parents.
- `live_size`: sum of observed live mapped store path sizes, with clear caveats.

For truthful treemap display, total numbers and rectangle metrics must be separated:

- Global totals should be de-duplicated by store path, so the total closure number is true.
- Rectangle area should use an explicit metric such as `added_size`, `nar_size`, or an attribution
  policy. It should not silently sum repeated transitive closure sizes.
- Each rectangle should expose ownership state: unique to this parent, shared by N parents, or
  heavily shared core dependency.
- The UI can encode ownership later with stripe density, border style, opacity, small badges, or a
  color secondary channel. For example: solid fill for unique dependencies, diagonal hatch for
  shared dependencies, and a badge like `x7` for seven top-level owners.
- Details should show immediate parents, top-level owners, reverse-ref count, unique bytes, shared
  bytes, and attribution policy.

Treemap metric switching must make the active metric explicit.

### 9.4 Live Process Scan

The live process scan is a separate layer, not a replacement for Nix closure extraction.

Collection steps:

- Enumerate `/proc/<pid>` entries.
- Read `/proc/<pid>/exe` where permitted.
- Parse `/proc/<pid>/maps` for mapped files under `/nix/store` where permitted.
- Optionally map PIDs to systemd units using `systemctl` or cgroup paths.
- Record permission failures and processes that exit during scan.

This produces a snapshot. It does not prove that unobserved closure paths are unused; they may be
inactive commands, conditionally loaded plugins, services that start later, or data files.

### 9.5 Native Treemap App

The first UI is a minimal native app that opens a scan database by path. It is not responsible for
running the full scan pipeline. It should treat the SQLite artifact as the source of truth and keep
all expensive Nix operations behind explicit actions.

The preferred app implementation is Rust with GPUI, matching the CLI language and avoiding a split
Rust/TypeScript stack. Do not over-optimize rendering before the data model is proven. The first UI
can use straightforward GPUI drawing and only add more specialized rendering if real scan artifacts
show a need.

Minimum viable UI:

- Open a SQLite scan file from an argument or file picker.
- Show scan metadata and health: root, host, Nix version, scan date, node/edge/source counts, and
  unknown counts.
- Render one treemap view.
- Provide hierarchy selector and metric selector.
- Provide search.
- Show a details panel for the selected rectangle.
- Offer a targeted "explain why" action for selected store paths.

Later UI additions:

- Focused graph pane.
- Scan diff view.
- Live process overlay.
- Source and license report panes.

Core interactions:

- Breadcrumb drill-down through the selected hierarchy.
- Search by package name, store path, source URL, license, language, or process name.
- Switch size metric: NAR size, closure size, added size, source LOC, source unit count, reverse ref
  count, live mapped size.
- Switch color metric: category, language, license, source confidence, live/not-live, binary-only,
  flake input, risk class.
- Click a rectangle to open details.
- Toggle tiny nodes into aggregated `other` buckets until zoomed.
- Pin a node and run or load a cached `why-depends` explanation.

Details panel:

- Store path and derivation path.
- Runtime references and reverse references.
- Size metrics and attribution explanation.
- Source units, LOC policy, confidence, and language breakdown.
- License/source availability where known.
- Live process mappings if observed.
- Links to raw Nix JSON and commands used.

Future graph panel:

- Focused ego network around selected node.
- Runtime deps, reverse refs, or shortest path from system root.
- Build-time derivation neighborhood.
- Highlight treemap selection in graph and graph selection in treemap.

### 9.6 Hierarchies

Phase 1 hierarchies:

- `system -> top-level direct reference -> transitive package/path`
- `category -> package/output`

Phase 2 hierarchies:

- `source confidence -> source kind -> package`
- `language -> source unit -> package`
- `live process -> mapped store path`
- `systemd unit -> executable/library store paths`
- `license -> package`
- `flake input -> package/output`

Phase 3 hierarchies:

- `NixOS option/module -> package/output` where provenance can be inferred.
- `service role -> package/output` for custom hand-authored host semantics.

## 10. Technical Decisions

| Decision | Choice | Rationale | Status |
|----------|--------|-----------|--------|
| Runtime graph source | `nix path-info` plus `nix-store --query --references` | Supported CLI, JSON sizes, explicit edge extraction | Proposed |
| Build graph source | `nix derivation show -r` | Nix-native recursive derivation JSON, loaded once per newly discovered derivation closure and cached in memory | Resolved |
| Why explanations | Lazy `nix why-depends --precise` | Expensive but high-value details only when needed | Proposed |
| Scan root model | Any Nix installable/store path, with package-sized flake outputs as MVP target and NixOS toplevel outputs as scale target | Lets the same path scan waybap/klocc before full system closures | Resolved |
| Source LOC truth model | Confidence-tagged measurements | Exact universal source LOC is not available from Nix metadata | Proposed |
| Default LOC counter | `tokei` | Mature language detection and LOC accounting while the artifact/schema work is still moving quickly | Resolved |
| App boundary | Native app opens SQLite file | Keeps scan pipeline scriptable and UI minimal; avoids local web server as core product | Resolved |
| Implementation language | Rust for CLI and later native app | Single-language codebase, good CLI/process/SQLite story, aligns with existing klocc repo | Resolved |
| Native UI framework | GPUI preferred for later app | Rust-native UI path; defer until CLI artifact works | Resolved |
| Treemap rendering | Simple GPUI-native rendering first, optimize only after real artifacts demonstrate need | Avoid premature rendering complexity while keeping ownership-aware metrics in the artifact | Resolved |
| Future graph model | Defer until graph UI is actually in scope | Avoid pre-optimizing or locking into a JS graph stack before the Rust/GPUI app exists | Proposed |
| Future graph renderer | Defer until focused graph panes are actually in scope | Treemap artifact correctness matters first; graph rendering can be selected with real data later | Proposed |
| Storage | SQLite | Durable local artifact, easy CLI stats and direct app loading | Resolved |
| Nix internals | Avoid direct store DB/libstore at MVP | Supported CLIs reduce version-coupling risk | Proposed |
| Missing source realization | Automatic for discovered source store paths | Complete source accounting requires realizing source paths; this realizes sources, not arbitrary package build outputs | Resolved |
| Scan mode | One complete path | Runtime-only/source-only results are filters over the complete artifact; no shortcut scan profiles | Resolved |
| Machine-readable output | SQLite only | Additional JSON output duplicates the artifact format and is explicitly out of scope | Wontfix |
| LOC cache identity | Nix store path plus counter policy first | Follows Nix identity semantics and makes rescans incremental without content-hashing every tree first | Resolved |
| Derivation graph storage | Full normalized derivation graph | Required to explain which derivation introduced each source/build dependency | Resolved |

## 11. Implementation Phases

### Phase 1 -- Minimal Shippable CLI Artifact

Scope:

- [ ] CLI accepts any Nix installable or store path, with package-sized flake outputs as the first
  test target.
- [ ] Extract runtime closure nodes with size and closure size.
- [ ] Extract runtime reference edges.
- [ ] Store scan results and rollups in normalized SQLite.
- [ ] Populate schema metadata, scan, command, store path, runtime edge, runtime rollup, category,
  ownership rollup, and hierarchy tables.
- [ ] Provide `stats` command with limited totals, top contributors, and unknown counts.
- [ ] Provide `check` command for schema and scan-health validation.
- [ ] Run `nix why-depends --precise` lazily through a targeted command/action for selected
  root/path pairs and cache the result.
- [x] Count source LOC and language LOC for discovered source units.

Success criteria:

- Scanning the klocc repo's own flake package succeeds and writes a valid SQLite artifact.
- Scanning another package-sized flake output such as waybap succeeds through the same command path
  if the repo is available.
- Scanning `/run/current-system` is allowed as a stretch validation but not required for the first
  MVP.
- Scanning a KittyOS host flake installable succeeds or reports exact missing evaluation/build
  requirements as a later scale test.
- CLI stats can answer the top 20 store paths by NAR size, closure size, reverse reference count,
  and owner count.
- Selecting a package shows direct references, reverse references, and a `why-depends` path from
  the root.
- The scan artifact can be closed, reopened, checked, summarized, and viewed without rerunning Nix.

### Phase 2 -- Source Provenance And First LOC Counts

Scope:

- [x] Parse `nix derivation show -r` into derivation nodes.
- [x] Map output paths to derivations when possible.
- [x] Identify high-confidence source units from fixed-output derivations, `env.src`, and
  `inputs.srcs`.
- [x] Count realized and realizable source units with `tokei`.
- [x] Emit confidence-tagged source and LOC rollups.
- [x] Traverse the full discovered derivation build graph without an arbitrary depth limit.
- [x] Deduplicate derivation expansion, source units, and per-source LOC measurements during a scan.
- [x] Persist the full normalized derivation graph, including derivations, outputs, input
  derivations, input source paths, source links, builder/system metadata, and raw derivation JSON.
- [x] Add honest scan-health metrics for derivations expanded, source candidates found, unknown
  source derivations, realization status, LOC cache hits/misses, and scan timing.
- [x] Add runtime/build-time and unique/shared source LOC rollups for filtering and treemap toggles.
- [ ] Add treemaps for `source confidence`, `source kind`, and `language`.

Success criteria:

- The UI can separate counted source LOC, patch LOC, unknown source units, and binary-unavailable
  units.
- Every displayed LOC number links to counter policy and source unit list.
- Missing derivers are visible, not silently ignored.

### Phase 3 -- Minimal Native GPUI Treemap App

Scope:

- [ ] Before any GPUI implementation, inspect current GPUI source/examples under a fresh or updated
  Zed checkout. Required references: `crates/gpui/README.md`, `crates/gpui/docs/contexts.md`,
  `crates/gpui/examples/hello_world.rs`, `crates/gpui/examples/painting.rs`, `App::prompt_for_paths`,
  and `PathPromptOptions`.
- [ ] Provide GPUI native app that accepts a SQLite path and opens a system file picker.
- [ ] Keep the native app as a viewer only; it must not run Nix scans.
- [ ] Strictly validate current schema on open. No migrations or compatibility views in first UI.
- [ ] First screen is a simple artifact opener: manual path field plus file picker action.
- [ ] After loading, show the treemap directly, not a dashboard interstitial.
- [ ] Render the treemap with `canvas(...)`/window paint primitives, not thousands of child elements.
- [ ] Precompute treemap rectangles and hit-test data so hover, left-click drilldown, and right-click
  back navigation update within a single frame for already-loaded data.
- [ ] Default area metric is source code LOC.
- [ ] Default color encoding is runtime-linked versus build-time-only.
- [ ] Top controls include metric selection plus runtime/build/unique/shared filtering.
- [ ] Left hover panel shows details for the rectangle under the mouse.
- [ ] Use Catppuccin Frappe visual styling.

Success criteria:

- Treemap app can load the generated SQLite file directly and render package-sized and larger
  artifacts without UI lockup.
- Rectangle area, color, active hierarchy, active metric, and active filters are explicitly labeled so
  the treemap is truthful.
- Left click drills into direct dependencies; right click navigates back.
- Hover panel explains source kind, LOC, runtime/build layer, generated/fixed-output status, and
  derivation provenance where available.

### Phase 4 -- Missing Source Fetching And Ecosystem Classification

Scope:

- [x] Realize missing source store paths discovered from recursive derivation metadata.
- [ ] Detect Cargo, npm, Go, Python, JVM, and other ecosystem dependency bundles.
- [ ] Add policy toggles for vendored deps, generated/minified code, tests, docs, and examples.
- [x] Deduplicate source units by store path when available.
- [x] Cache LOC counts by source path and counter policy within a scan.
- [x] Persist LOC counts between scans keyed by Nix store path and counter policy.
- [x] Explore and implement stronger source inference heuristics for derivations whose metadata does
  not expose an obvious source candidate.

Success criteria:

- Re-running a scan should eventually reuse persistent cached LOC counts when source identity and
  policy are unchanged.
- Unknown source rows are explicitly summarized and traced back to derivations so inference gaps are
  visible and actionable.
- Vendored dependency LOC can be included/excluded without rescanning all runtime data.
- Source realization never builds arbitrary package derivations unless a future explicit flag is
  added.

### Phase 5 -- Live Process Snapshot

Scope:

- [ ] Add `/proc` scanner for executable and mapped store paths.
- [ ] Map PIDs to process names and systemd units where possible.
- [ ] Add live process treemap hierarchy.
- [ ] Cross-highlight live paths inside the full runtime closure.

Success criteria:

- UI can distinguish live observed store paths from installed but inactive store paths.
- Permission failures and exited processes are reported as scan visibility states.
- The live process layer does not claim unobserved paths are unused.

### Phase 6 -- Advanced Attribution And NixOS Semantics

Scope:

- [ ] Add flake input attribution where possible.
- [ ] Explore NixOS option/module provenance for packages and services.
- [ ] Add systemd service-level semantic grouping.
- [ ] Add diff mode between two scans or generations.
- [ ] Add export reports for closure bloat, source-unavailable artifacts, and largest LOC sources.

Success criteria:

- Comparing two generations shows added/removed store paths, size deltas, and source LOC deltas.
- Service-oriented views explain which closure portions are attributable to desktop, server,
  gaming, agent tooling, container services, etc., where reliable evidence exists.

## 12. Non-Goals

- Perfectly count every authored line of source code behind a system. Nix does not expose enough
  semantic information for that universally.
- Treat runtime closure as proof of current execution. Runtime closure means required/available for
  the system output, not currently running.
- Build arbitrary packages just to discover source.
- Replace `nix-tree`, `nix-du`, or `nix why-depends` for terminal workflows.
- Provide a global public web service. This is a local analysis tool over local Nix state.
- Solve supply-chain trust or vulnerability scanning in the MVP. Those can be later overlays.
- Attribute every package to an exact NixOS option/module in early phases.

## 13. Open Questions

1. **Analytical engine:** Is SQLite alone enough, or should DuckDB be considered later for heavier
   rollups while keeping SQLite as the portable artifact?
2. **LOC policy default:** Should vendored dependencies be excluded by default, included by default,
   or shown as a separate always-visible metric?
3. **Source realization:** Is opt-in fixed-output source fetching acceptable, or should Phase 2 stay
   strictly no-network/no-realization?
4. **Live process permissions:** Should the tool ever ask for root to read all `/proc/<pid>/maps`,
   or should it operate only at user-readable visibility?
5. **Package identity:** What is the canonical grouping key for store paths: derivation name,
   `pname/version` from metadata, output path name, or an inferred package record?
6. **NixOS semantics:** Is module/option attribution a must-have for first usefulness, or a later
   research track after closure/source views work?
7. **Sharing artifacts:** Should scan databases be considered private because they include local
    process names, paths, and possibly source URLs?

## 14. Resolved Questions

| Question | Answer | Date | Evidence/Rationale |
|----------|--------|------|--------------------|
| Is Nix runtime closure the same as derivation/source graph? | No. Runtime closure and build-time derivation graph are separate layers and must be modeled separately. | May 20, 2026 | Nix docs and local `nix derivation show --help` describe `.drv` files as build-time dependency graph; `nix-store --query` distinguishes output closures from derivation closures. |
| Can source LOC be exact for every system dependency? | No. The tool should report confidence-tagged counted source LOC with explicit unknown and unavailable buckets. | May 20, 2026 | Derivers can be unknown; source is conventionally inferred from derivation env/fetchers; binary-only and generated artifacts cannot be universally counted. |
| Should the first UI force the Nix DAG into one tree? | No. Treemaps use derived hierarchies; the underlying dependency graph remains a DAG. | May 20, 2026 | Runtime references are shared; naive tree projection double-counts and hides sharing. |
| Closest existing UX reference? | `nix-tree`. | May 20, 2026 | It already supports closure browsing, search, sorting, closure/NAR/added size, and why-style interactions, but not a native treemap artifact explorer or source LOC. |
| Preferred visualization foundation? | Rust + GPUI for the later native app, with simple rendering first and optimization only after real artifacts show a need. | May 20, 2026 | User clarified preference for Rust for both CLI and UI, GPUI from Zed, and avoiding premature rendering optimization. |
| Product boundary? | CLI generates and processes the scan, stores normalized SQLite, prints limited stats/totals, and a native app opens the SQLite file for minimal treemap exploration. | May 20, 2026 | User clarified the desired design: CLI for generation/processing/stats, normalized storage, native minimal UI loading the file by path. |
| Is SQLite just a cache? | No. SQLite is the normalized artifact and interchange boundary between CLI processing, stats, native app viewing, and future diff/report tools. | May 20, 2026 | User explicitly asked for normalized SQLite storage or equivalent, with the native app taking a SQLite path as input. |
| Can the scanner target software smaller than a NixOS host? | Yes. The root should be any Nix installable or store path; NixOS hosts are scanned via their `config.system.build.toplevel` derivation, while package PoCs use flake package outputs such as klocc or waybap. | May 20, 2026 | User asked for waybap/klocc package PoCs before full NixOS system scale. |
| How should shared dependencies appear in treemaps? | Totals must be de-duplicated; rectangle area must name its metric; ownership overlays should show whether a dependency is unique, shared, or heavily shared by many parents/owners. | May 20, 2026 | User requested truthful treemap display with visual distinction for unique versus multi-owner dependencies. |
| Preferred implementation stack? | Rust CLI now, Rust GPUI native app later. | May 20, 2026 | User preference; also aligns with the existing klocc Rust repo and avoids a split web stack. |

## 15. References

- Nix `path-info`: https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-path-info
- Nix `derivation show`: https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-derivation-show
- Nix derivation JSON format: https://nix.dev/manual/nix/latest/protocols/json/derivation/
- Nix `why-depends`: https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-why-depends
- Nix store query: https://nix.dev/manual/nix/latest/command-ref/nix-store/query
- Nixpkgs fetchers: https://nixos.org/manual/nixpkgs/stable/#chap-pkgs-fetchers
- `nix-tree`: https://github.com/utdemir/nix-tree
- `nix-du`: https://github.com/symphorien/nix-du
- `nix-visualize`: https://github.com/craigmbooth/nix-visualize
- `nix-query-tree-viewer`: https://github.com/cdepillabout/nix-query-tree-viewer
- `nix-output-monitor`: https://github.com/maralorn/nix-output-monitor
- Tvix: https://github.com/tvlfyi/tvix
- `scc`: https://github.com/boyter/scc
- `tokei`: https://github.com/XAMPPRocky/tokei
- `cloc`: https://github.com/AlDanial/cloc
- GPUI: https://github.com/zed-industries/zed/tree/main/crates/gpui
