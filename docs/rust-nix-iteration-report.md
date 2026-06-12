# Rust + Nix Iteration And Cache Report

Date: May 26, 2026

## Executive Summary

The practical answer is to keep Rust, keep Nix, but stop treating Nix package/check builds as the inner edit loop.

Use this split:

| Loop | Tool | Purpose | Expected behavior |
| --- | --- | --- | --- |
| Inner loop | `nix develop -c cargo check/test/build` | Fast feedback while editing | Reuses local `target/`, Cargo incremental compilation, optional `sccache` |
| Integration loop | `nix build .#packages...` | Reproducible package proof | Rebuilds a fresh sandboxed derivation when source changes, but reuses dependency artifacts |
| Proof loop | `nix build .#checks...` | VM snapshots, scanner artifacts, full validation | Slow by design; cacheable and reproducible, not interactive |

This repo already made the right high-level choice by using `crane` with package-specific `buildDepsOnly` derivations. The main pain comes from asking Nix to run expensive release builds and VM checks during normal iteration. The highest-leverage improvements are workflow and check-shape changes, not switching away from `crane`.

Recommended direction:

| Priority | Recommendation | Why |
| --- | --- | --- |
| P0 | Use `cargo check -p <crate>` and targeted `cargo test -p <crate>` inside `nix develop` as the default edit loop | This is the only path that reuses Cargo incremental state without copying sandbox artifacts around |
| P0 | Keep `crane`; do not switch to `buildRustPackage`, `naersk`, `cargo2nix`, or `crate2nix` without a measured bottleneck | `crane` already gives the best ergonomics/cache tradeoff for this workspace |
| P1 | Add explicit fast checks for common work, separate from VM snapshots | Avoid using `gui-snapshot-*` as a compile/test smoke check |
| P1 | Keep GUI live runners able to use `KLOCC_GUI_BIN=target/debug/klocc-gui` or `target/release/klocc-gui` | Lets GPUI work iterate through Cargo while still using the same Weston harness |
| P1 | Push or share Nix outputs through a binary cache if multiple machines/agents repeat builds | Nix cache hits only help when the exact store path is available |
| P2 | Consider `sccache` only for the Cargo inner loop, not as the main Nix optimization | Nix binary substitution generally beats compiler-cache tricks for Nix builds |
| P2 | Consider `crate2nix`/`cargo2nix` only if per-crate Nix cache granularity becomes a measured need | More granular cache, but more eval and maintenance complexity |

## Current Repo Shape

Relevant files:

| File | Observation |
| --- | --- |
| `flake.nix` | Uses `fenix` stable toolchain and `crane` via `craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain` |
| `flake.nix` | Uses `cargoSrc = craneLib.cleanCargoSource ./.`, which filters non-Cargo files and avoids rebuilds from unrelated docs/Nix files |
| `flake.nix` | Splits `kloccTestCargoArtifacts`, `kloccdCargoArtifacts`, `kloccCargoArtifacts`, and `kloccGuiCargoArtifacts` via `craneLib.buildDepsOnly` |
| `flake.nix` | Builds `klocc`, `kloccd`, and `kloccGui` via `craneLib.buildPackage` with package-specific `cargoExtraArgs` |
| `nix/unit/default.nix` | Unit/property check reuses `kloccTestCargoArtifacts` and runs `craneLib.cargoTest -p klocc` |
| `nix/snapshot/default.nix` | Snapshot checks dynamically create VM checks for every scenario/environment pair |
| `scripts/gui-wayland-live.sh` | Already supports `KLOCC_GUI_BIN`, so the live GUI runner can use a Cargo-built binary instead of a Nix-built binary |

Pinned tool versions from `flake.lock`:

| Input | Revision |
| --- | --- |
| `crane` | `edb38893982a3338972bb4a2ec7ce7c29ba10fd9` |
| `fenix` | `b7bd9323fe26a3b4f4bddbb2c2a1dacabced2f88` |
| `nixpkgs` | `d233902339c02a9c334e7e593de68855ad26c4cb` |

Current measured/observed facts from this workspace:

| Command | Observation |
| --- | --- |
| `nix build .#packages.x86_64-linux.klocc --dry-run` | No output, meaning already locally available at the time measured |
| `nix build .#checks.x86_64-linux.gui-snapshot-metric-runtime-drilldown-weston --dry-run` | No output after the latest build, meaning current snapshot output was locally available |
| `nix path-info --recursive --size --closure-size --human-readable .#packages.x86_64-linux.klocc-gui` | `klocc-gui` runtime output was about `29.3 MiB`, closure about `143.9 MiB` |
| Parallel Nix evals | Produced `SQLite database ... eval-cache ... is busy` warnings, harmless but a sign not to over-parallelize repeated evals in scripts |

## Why Nix Builds Feel Slow For Rust

Nix is not slow because it cannot cache. It is slow in the edit loop because its unit of cache is a derivation output, not a mutable Cargo `target/` directory.

When a Rust source file changes:

| Layer | What happens |
| --- | --- |
| Flake evaluation | Nix evaluates the flake again unless the eval cache can reuse a matching flake version |
| Source path | `cleanCargoSource` produces a new source store path because Rust source changed |
| Dependency artifact derivation | With `crane.buildDepsOnly`, dependencies normally do not rebuild if manifests/lock did not change |
| Package derivation | The package derivation changes and rebuilds in a clean sandbox |
| Cargo build | Cargo can reuse copied artifacts from `cargoArtifacts`, but it is still a fresh derivation and typically a release build |
| VM checks | Snapshot checks then boot QEMU/compositor/app and run screenshots, which dominates after compile cache hits |

This means Nix gives strong replayability, but it cannot match the latency of `cargo check` in a persistent working directory.

## How Nix Cache Actually Works

Nix has several cache-like mechanisms. They solve different problems.

| Mechanism | Caches | Helps with | Does not help with |
| --- | --- | --- | --- |
| Local `/nix/store` | Exact store paths already built or substituted | Re-running the same derivation | New derivation paths after source/flag/env changes |
| Binary substituter | Exact store paths from remote cache | Sharing builds across machines/agents | Dirty or unique local derivations not uploaded |
| Flake eval cache | Evaluation result for a particular flake version | Repeated eval of same flake output | Build time, changed derivations, intermediate eval subexpressions |
| Fixed-output derivations | Content-addressed fetch/vendor outputs | Reusing crates/source fetches when hashes match | Reusing compiled Rust code |
| `crane.buildDepsOnly` cargo artifacts | A compressed/reused Cargo `target` from dependency-only build | Avoiding dependency recompiles after app source edits | Avoiding app/local-crate recompiles |
| Cargo `target/` | Incremental compiler artifacts in the working tree | Fast inner-loop rebuilds | Nix sandbox/package reproducibility |
| `sccache` | Individual rustc invocations | Repeated local/CI compiles with stable paths/env | Binary crates/link steps; Nix derivation path churn by itself |

Important Nix facts:

| Fact | Consequence |
| --- | --- |
| Normal derivation output paths are determined by the derivation specification and inputs | Any changed `src`, flags, env, toolchain, dependency path, `pname`, `version`, or builder command can force a new output path |
| Fixed-output derivations are content-addressed by declared output hash/name | Fetch/vendor steps can be reused even when fetch command details differ, as long as output hash/name stays stable |
| Substituters only work for exact store paths | A remote cache cannot help if the current dirty source generated a unique path that was never uploaded |
| IFD pauses evaluation to build/read a derivation output | Avoid IFD in hot paths unless the cost is justified |
| `recursive-nix` lets builders call Nix | Useful for this scanner domain, but it complicates cache reasoning and should stay out of fast checks |

## Rust-on-Nix Tool Comparison

| Tool | Cache granularity | Eval cost | Best use | Tradeoff |
| --- | --- | --- | --- | --- |
| `crane` | Dependency artifact derivation plus package/check derivations | Low/medium | Workspaces that want Cargo-native behavior and good Nix checks | Coarser than per-crate systems, but much simpler |
| `rustPlatform.buildRustPackage` | Vendor/fetch deps plus one Cargo package derivation | Low | Packaging one Rust app in nixpkgs style | Less ergonomic for many package-specific checks/artifact reuse |
| `naersk` | Dependency derivation plus main derivation | Low | Simple Rust projects | Less explicit and less check-composable than `crane` |
| `cargo2nix` | One derivation/function per crate | Medium/high | CI/cache-heavy projects that accept generated Nix and overrides | More eval surface, generated `Cargo.nix`, git dependency caveats |
| `crate2nix` | One derivation per crate via `buildRustCrate` | Medium/high | Per-crate Nix incrementality, especially in CI | Generated file maintenance or IFD, crate override burden |
| `dream2nix` Rust modules | Varies by module | Medium/unknown | Multi-language framework standardization | Rust support is more experimental; not a speed-first choice here |
| `cargo-chef` | Docker dependency layer | Outside Nix | Docker builds | Duplicates `crane.buildDepsOnly` concept for Nix workflows |
| `sccache` | rustc invocation cache | Outside Nix eval | Local Cargo loop or CI with remote cache | Rust cache limitations; not a substitute for Nix binary caches |

Conclusion for this repo: keep `crane`.

The current repo uses GPUI git dependencies, native GUI libraries, multiple binaries, VM tests, and a recursive scanner artifact. A per-crate Nix system might improve remote cache reuse for some dependency graph changes, but it would add generated files and per-crate override complexity exactly where GUI/native crates tend to be fiddly.

## What To Do In This Repo

### 1. Use Cargo For Inner Loops

Default edit loop for CLI/scanner code:

```sh
nix develop -c cargo check -p klocc
nix develop -c cargo test -p klocc
```

Default edit loop for GUI code:

```sh
nix develop -c cargo check -p klocc-gui
nix develop -c cargo build -p klocc-gui
```

Run the live GUI with the Cargo-built binary:

```sh
KLOCC_GUI_BIN=target/debug/klocc-gui nix develop -c klocc-gui-wayland-live start /path/to/artifact.sqlite
```

Use release locally only when testing performance-sensitive behavior:

```sh
nix develop -c cargo build --release -p klocc-gui
KLOCC_GUI_BIN=target/release/klocc-gui nix develop -c klocc-gui-wayland-live start /path/to/artifact.sqlite
```

### 2. Keep Nix Package Builds For Reproducibility Proofs

Use these after the code-level loop passes:

```sh
nix build .#packages.x86_64-linux.klocc -L
nix build .#packages.x86_64-linux.klocc-gui -L
nix build .#checks.x86_64-linux.klocc-unit-property-tests -L
```

### 3. Use Snapshot Checks As Outer Proofs

VM snapshot checks are inherently slow because they include package builds, artifact generation, QEMU, compositor startup, input driving, screenshots, and image assertions.

Use them when validating behavior, not on every source edit:

```sh
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston -L
nix build .#checks.x86_64-linux.gui-snapshot-metric-runtime-drilldown-weston -L
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland -L
nix build .#checks.x86_64-linux.gui-snapshot-metric-runtime-drilldown-hyprland -L
```

### 4. Add Or Preserve Fast Non-VM Checks

The repo should have checks that catch most Rust regressions without booting a VM.

Good candidates:

| Check | Shape | Why |
| --- | --- | --- |
| CLI unit/property tests | Existing `klocc-unit-property-tests` | Fast scanner correctness proof |
| GUI compile check | `craneLib.cargoClippy` or `cargoCheck` for `-p klocc-gui` | Catches GPUI/Rust errors without VM |
| Layout unit/property tests | Cargo tests for `klocc-gui` layout module | Tests visual layout logic without compositor |
| Artifact fixture check | Run GUI data loading/layout on a prebuilt or small checked-in fixture | Avoid scanner + recursive Nix cost for basic GUI logic |

### 5. Treat Binary Cache As The Nix Acceleration Path

For Nix builds, the strongest acceleration is a substituter containing the exact output paths.

If multiple agents or machines repeat the same builds, cache these outputs:

| Output type | Worth caching? | Reason |
| --- | --- | --- |
| `kloccCargoArtifacts`, `kloccGuiCargoArtifacts`, test cargo artifacts | Yes | Expensive dependency builds; stable across source edits |
| `klocc`, `klocc-gui` package outputs | Yes | Useful across agents on same commit/source hash |
| `guiScreenshotArtifact` | Yes | Avoids rerunning scanner for snapshots |
| VM snapshot outputs | Maybe | Large but useful for exact scenario review reuse |

The cache only helps if the derivation path matches. Dirty local changes generate paths no one else has.

### 6. Use `sccache` Only Deliberately

`sccache` can be useful for direct Cargo work after adding `sccache` to the dev shell or otherwise putting it on `PATH`:

```sh
nix develop -c env RUSTC_WRAPPER=sccache cargo check -p klocc-gui
```

But it has Rust caveats:

| Caveat | Impact |
| --- | --- |
| `rustc` incremental must be disabled for cached invocations | Conflicts with Cargo's strongest local dev optimization unless tuned carefully |
| Binary/linking crates are not cached | Final binaries still link normally |
| Proc macros and filesystem-reading build scripts can miss or avoid cache hits | Less reliable for complex GUI/native graphs |
| Nix derivation env/path changes alter cache keys | Less useful than exact Nix binary substitutes for Nix builds |

I would not add `sccache` to Nix package derivations first. Try it in the dev shell only if `cargo check/build` remains too slow after normal incremental reuse.

## How To Diagnose Rebuilds

Use these commands before guessing why Nix rebuilt something:

```sh
# What would build or substitute?
nix build .#packages.x86_64-linux.klocc-gui --dry-run

# Get a derivation path.
nix path-info --derivation .#packages.x86_64-linux.klocc-gui

# Dump derivation JSON.
nix derivation show "$(nix path-info --derivation .#packages.x86_64-linux.klocc-gui)"

# Compare two derivations.
nix shell nixpkgs#nix-diff -c nix-diff /nix/store/old.drv /nix/store/new.drv

# Inspect closure size.
nix path-info --recursive --size --closure-size --human-readable .#packages.x86_64-linux.klocc-gui

# Why does a runtime closure contain something?
nix why-depends .#packages.x86_64-linux.klocc-gui nixpkgs#openssl --precise

# Why does a build-time derivation depend on something?
nix why-depends --derivation .#packages.x86_64-linux.klocc-gui nixpkgs#pkg-config

# Disable flake eval cache when measuring evaluation.
nix build .#packages.x86_64-linux.klocc-gui --no-link --option eval-cache false

# Detect IFD sensitivity.
nix build .#packages.x86_64-linux.klocc-gui --no-link --option allow-import-from-derivation false
```

Useful Nix settings to know:

| Setting | Meaning |
| --- | --- |
| `eval-cache` | Reuses flake evaluation for a particular flake version; does not cache builds |
| `substituters` | Binary cache URLs to query |
| `trusted-public-keys` | Required trust roots for substituters |
| `max-jobs` | Number of parallel Nix build jobs |
| `cores` | `NIX_BUILD_CORES` passed into each build |
| `keep-outputs` | Keeps build-time outputs reachable from derivations, useful but disk-expensive for dev machines |
| `narinfo-cache-negative-ttl` | Negative cache hit TTL; can make just-uploaded substitutes appear missing until refreshed |

## Decision Matrix

| Problem | First thing to try | Avoid initially |
| --- | --- | --- |
| Rust source edits are slow | `nix develop -c cargo check -p ...` | `nix build` on every edit |
| GUI behavior needs visual inspection | Cargo-built binary through `klocc-gui-wayland-live` | Full Hyprland+Weston matrix every edit |
| CI repeats dependency builds | Binary cache `buildDepsOnly` outputs | Rewriting build system immediately |
| Nix eval is slow | Measure with `--option eval-cache false`; avoid IFD; avoid generated huge attrsets unless needed | Moving to per-crate generated Nix without measuring |
| App package rebuilds too much | Ensure source filtering and package-specific checks are tight | Putting more files into `src` |
| Cargo local compile still slow | Try `cargo check`, fewer features/packages, optional `sccache`, faster linker | Forcing every loop through sandboxed release Nix |
| Need per-crate CI cache | Evaluate `crate2nix`/`cargo2nix` in a branch and compare eval/build/cache metrics | Migrating before proof |

## Sources Consulted

Authoritative docs/source:

| Topic | Source |
| --- | --- |
| `crane` API, `buildDepsOnly`, `buildPackage`, `cleanCargoSource`, `cargoArtifacts` | `https://raw.githubusercontent.com/ipetkov/crane/master/docs/API.md` |
| Nixpkgs Rust packaging, `buildRustPackage`, `cargoHash`, `cargoLock`, `importCargoLock`, `buildRustCrate` | `https://raw.githubusercontent.com/NixOS/nixpkgs/master/doc/languages-frameworks/rust.section.md` |
| `naersk` dependency split and options | `https://raw.githubusercontent.com/nix-community/naersk/master/README.md` |
| `cargo2nix` generated per-crate DAG and workspace behavior | `https://raw.githubusercontent.com/cargo2nix/cargo2nix/master/README.md` |
| `crate2nix` per-crate derivations and manual vs IFD generation | `https://raw.githubusercontent.com/nix-community/crate2nix/master/README.md` |
| Nix derivations and derivation input attributes | `https://nix.dev/manual/nix/latest/language/derivations` |
| Nix store derivation model | `https://nix.dev/manual/nix/latest/store/derivation/` |
| Nix config, eval cache, substituters, build parallelism, cache TTLs | `https://nix.dev/manual/nix/latest/command-ref/conf-file` |
| Import From Derivation | `https://nix.dev/manual/nix/latest/language/import-from-derivation` |
| `sccache` Rust caveats | `https://raw.githubusercontent.com/mozilla/sccache/main/docs/Rust.md` |

Local project files:

| File | Why it matters |
| --- | --- |
| `flake.nix` | Current `crane`/`fenix`, package split, dev shell, snapshot artifact generation |
| `nix/default.nix` | Check aggregation |
| `nix/unit/default.nix` | Existing `cargoTest` shape |
| `nix/snapshot/default.nix` | Scenario/environment check matrix |
| `scripts/gui-wayland-live.sh` | Existing `KLOCC_GUI_BIN` hook for Cargo-built GUI binaries |

## Final Recommendation

Do not try to make Nix feel like Cargo for the inner loop. That fights the model.

Make Cargo inside `nix develop` the fast loop, make `crane` package/check builds the reproducible proof, and make VM screenshots the outer acceptance test. If repeated Nix proofs are still painful across agents, invest in a binary cache for `buildDepsOnly`, package, artifact, and selected VM outputs before considering a migration to per-crate generated Nix.
