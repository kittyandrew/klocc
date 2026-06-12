# Rust + Nix Build Alternatives Benchmark

Date: May 26, 2026

## Recommendation

Stay on `crane`.

The deeper benchmark changed the conclusion from "probably keep `crane`" to "keep `crane`; `buildRustPackage` is not competitive for this GUI package's Nix edit/proof loop."

Best path forward:

1. Keep the current `crane` package structure.
2. Add explicit fast checks for `klocc-gui` so GUI compile/layout validation does not require VM snapshots.
3. Make `nix develop -c cargo check -p ...` the documented inner loop.
4. Keep VM snapshots as outer proof checks.
5. Invest in a binary cache for exact Nix outputs if multiple agents/machines repeat proof builds.
6. Do not migrate to `buildRustPackage`, `naersk`, `crate2nix`, or `cargo2nix` without a narrower measured reason.

## Final Ranking

| Rank | Option | Verdict | Confidence |
| --- | --- | --- | --- |
| 1 | Current `crane` | Keep | High |
| 2 | Cargo inside `nix develop` | Use as inner loop, not as package builder replacement | High |
| 3 | `crate2nix` | Possible future investigation for CI cache granularity only | Low/medium |
| 4 | `buildRustPackage` | Reject for this GUI package | High |
| 5 | `naersk` | Reject for now | Medium |
| 6 | `cargo2nix` | Reject for now | Medium |
| 7 | `sccache` | Do not add yet | Medium |

## Measurement Setup

Benchmarks used `hyperfine` in the current dirty workspace. That means absolute numbers are environment-specific, but the relative result for the two fully built candidates is strong enough to guide the next step.

Important caveats:

| Caveat | Handling |
| --- | --- |
| Existing worktree is dirty and broad-staged | Measurements represent current active development state, not a clean release commit |
| Nix store state is warm and has prior outputs | Null-build timings are intentionally warm-store timings |
| Some alternatives were not fully made production-ready | They are not ranked as migration-ready |
| Timestamp-only `touch` is not enough for Nix | A temporary one-line Rust comment was added and removed to force a real source hash change |
| Sample sizes are small | The controlled source-edit difference between `crane` and `buildRustPackage` is large enough to be actionable |

Temporary benchmark expression:

The experimental `buildRustPackage`/`naersk` expression was intentionally not retained under `nix/`: direct `nix/` children are reserved for adopted test methodologies or `shared`.

## Results

### Inner Cargo Loop

| Scenario | Command | Result |
| --- | --- | --- |
| Warm GUI check | `nix develop -c cargo check -p klocc-gui` | 14.299s mean over 2 runs |
| Timestamp-only GUI check | `touch crates/klocc-gui/src/layout.rs && nix develop -c cargo check -p klocc-gui` | 2.632s once |
| With `sccache` wrapper | `RUSTC_WRAPPER=sccache cargo check -p klocc-gui` through `nix develop` | 2.329s mean over 2 runs |
| With `sccache` after timestamp touch | same with touched `layout.rs` | 2.455s once |

`sccache` stats showed `0` cache hits and `0` executed compilations, with all requests non-cacheable (`missing input`, `crate-type`, or `-`). So the fast `sccache` numbers are not evidence that `sccache` helped; they mostly show the normal Cargo incremental/no-op path is already fast.

### Current `crane`

| Scenario | Result | Evidence |
| --- | --- | --- |
| Package eval | 8.088s mean over 3 | `crane-eval.txt` |
| Dry-run/null style check when already built | 554.5ms mean over 3 | `crane-dry-run.txt` |
| Primed null build | 551.9ms mean over 5 | `crane-null-build.txt` |
| Source-content edit rebuild | 35.412s once | `crane-source-edit-build.txt` |

Source-edit rebuild details:

| Detail | Result |
| --- | --- |
| Derivation rebuilt | only `klocc-gui-0.1.6.drv` |
| Dependency artifact reuse | yes, decompressed `klocc-gui-deps` cargo artifacts |
| Cargo build | compiled `klocc` and `klocc-gui` only |
| Cargo build time | 5.87s |
| Cargo test time | 4.63s compile plus 15 tests in 0.10s |
| Total wall time | 35.412s |

This is the behavior we want from a Nix proof build: source changes rebuild local crates, not the whole GPUI/wgpu/native dependency graph.

### `buildRustPackage`

Prototype shape:

| Field | Value |
| --- | --- |
| Toolchain | same pinned `fenix` stable components as project |
| Source | `craneLib.cleanCargoSource` from the workspace |
| Cargo lock handling | `cargoLock.lockFile = Cargo.lock`, `allowBuiltinFetchGit = true` |
| Package selection | `cargoBuildFlags = ["-p" "klocc-gui"]` |
| Check phase | disabled in prototype |
| Native deps | same GUI system deps and wrapper pattern |

| Scenario | Result | Evidence |
| --- | --- | --- |
| Eval | 9.309s mean over 3 | `build-rust-package-eval.txt` |
| Initial dry-run | 255 derivations would build | `build-rust-package-dry-run.txt` |
| First successful build | Cargo build phase 5m43s | `build-rust-package-build-error.log` despite misleading filename |
| Primed null build | 9.397s mean over 5 | `build-rust-package-null-build.txt` |
| Source-content edit rebuild | 364.077s once | `build-rust-package-source-edit-build.txt` |

Source-edit rebuild details:

| Detail | Result |
| --- | --- |
| Derivation rebuilt | one `klocc-gui-build-rust-package-bench-0.1.6.drv` |
| Dependency artifact reuse | no compiled dependency artifact layer equivalent to current `crane` setup |
| Cargo build | rebuilt full dependency graph, including GPUI/wgpu/native graph |
| Cargo build time | 5m44s |
| Total wall time | 364.077s |

This is the decisive result. Even though `buildRustPackage` is the standard nixpkgs packaging tool, it is a bad replacement for the current `crane` setup in this repo because it loses the `buildDepsOnly` cargo artifact split.

### `naersk`

| Scenario | Result |
| --- | --- |
| Eval | 10.759s mean over 3 |
| Dry-run | 1450 derivations would build |

Caveat: the quick prototype used an unpinned `fetchTarball` from `master`, so the measurement is not reproducible enough to rank precisely. It is still not promising: eval was slower than `crane`, and the dry-run shape was much larger than the current path.

Recommendation: do not pursue unless there is a separate reason to simplify away from `crane`. There is no speed evidence for it here.

### `crate2nix`

| Scenario | Result |
| --- | --- |
| Generate | 24.725s once |
| Generated `Cargo.nix` size | 1.2 MiB, 32,224 lines |
| Generated crate hash file | 5.2 KiB |
| Generated `klocc-gui` eval | 6.314s once |
| Dry-run | 1063 derivations would build before native override work |

`crate2nix` is the only alternative with a plausible unique advantage: per-crate Nix derivations could improve exact binary-cache reuse in CI after the store/cache is fully primed.

But it has costs:

| Cost | Impact |
| --- | --- |
| Generated 1.2 MiB `Cargo.nix` | More review noise and lockstep regeneration burden |
| Native/GUI overrides not solved | GPUI/wgpu/native deps likely need crate-specific fixes |
| Dry-run wants 1063 derivations | Per-crate granularity creates a much wider Nix graph |
| Eval result was only measured once | Not enough to justify migration |

Recommendation: keep as a future branch experiment only if CI/cache reuse becomes the measured bottleneck. Do not migrate now.

### `cargo2nix`

`cargo2nix` was not pursued beyond tool acquisition/help because even invoking the flake tool path pulled in a large independent Rust toolchain/build graph. The quick path listed 557 derivations for the tool route before even modeling this repo.

Recommendation: do not pursue now. It has the same generated/per-crate complexity class as `crate2nix`, with no evidence it would improve this repo's current pain.

## Adversarial Review Findings

A critic review challenged the first-pass conclusions. The important corrections were accepted:

| Finding | Resolution |
| --- | --- |
| Eval time is not the main bottleneck | Added controlled source-content rebuild benchmark |
| Dry-run derivation counts are not comparable across candidates | Counts are now diagnostic only, not ranking evidence |
| `naersk` prototype was unpinned | Marked as confounded; not used for final ranking beyond "not promising" |
| `crate2nix` eval is not migration evidence until it builds with overrides | Marked as future-only |
| `sccache` showed no actual cache hits | Do not add `sccache` yet |
| Sample sizes were small | Final decision relies on the large 35s vs 364s source-edit gap, not close-call timing |

The deeper `crane` vs `buildRustPackage` test was the most important follow-up and strongly favored `crane`.

## Plan Forward

### Phase 1: Lock In The Fast Loop

Add or document these as the normal local workflow:

```sh
nix develop -c cargo check -p klocc
nix develop -c cargo test -p klocc
nix develop -c cargo check -p klocc-gui
nix develop -c cargo build -p klocc-gui
```

For UI work, keep using the existing live runner with a Cargo-built binary:

```sh
KLOCC_GUI_BIN=target/debug/klocc-gui nix develop -c klocc-gui-wayland-live start artifact.sqlite
```

### Phase 2: Add Fast GUI Proof Checks

Add checks that validate GUI code without a VM:

| Check | Expected benefit |
| --- | --- |
| `klocc-gui` Cargo check/clippy via `crane` | catches compile/API regressions without VM startup |
| `klocc-gui` layout tests via `craneLib.cargoTest -p klocc-gui` | isolates treemap/layout correctness from compositor behavior |
| fixture-backed artifact/layout test | validates viewer data path without recursive scanner + VM |

### Phase 3: Keep Snapshot Checks As Acceptance Tests

Do not optimize the inner loop around VM snapshots. They validate a broad chain:

| Snapshot cost component | Why it remains |
| --- | --- |
| Nix package build | proof that packaged binary works |
| scanner artifact | proof against realistic artifact data |
| QEMU/compositor startup | proof of Wayland/Hyprland/Weston behavior |
| screenshot capture/assertions | visual regression proof |

Use snapshots before merging/releasing or after meaningful UI behavior changes, not on every edit.

### Phase 4: Binary Cache For Repeated Proof Work

If multiple agents or machines repeat Nix proof builds, cache these exact outputs:

| Output | Priority |
| --- | --- |
| `kloccGuiCargoArtifacts` / `kloccCargoArtifacts` | highest |
| package outputs | high |
| `guiScreenshotArtifact` | high for snapshot work |
| selected VM snapshot results | medium, only if storage cost is acceptable |

Success metric: another machine should substitute most proof outputs instead of building them.

### Phase 5: Only Revisit Per-Crate Nix If CI Demands It

Revisit `crate2nix`/`cargo2nix` only if all are true:

1. CI/proof builds are still too slow after binary caching `crane` outputs.
2. The bottleneck is dependency graph rebuild granularity, not VM runtime.
3. A branch proves `klocc-gui` builds with required native overrides.
4. A branch proves source-edit rebuilds and cross-agent cache reuse beat `crane` enough to justify generated-file maintenance.

## Success Metrics

Use these to measure improvement after implementing the plan:

| Metric | Target |
| --- | --- |
| Warm `cargo check -p klocc-gui` after local edit | low single-digit seconds when incremental state is warm |
| `crane` source-edit Nix proof for `klocc-gui` | dependencies not rebuilt; local crates only |
| `klocc-gui` fast non-VM check | avoids VM startup and catches compile/layout regressions |
| Snapshot checks | unchanged or improved reliability; not used as inner loop |
| Remote/cache worker proof | mostly substitutions for cargo artifacts/packages/artifacts |

## Bottom Line

The current `crane` setup is doing the key thing correctly: it preserves a dependency artifact layer. A real source edit rebuilt the `crane` GUI package in 35.4s, while the `buildRustPackage` prototype rebuilt the full dependency graph in 364.1s.

The next improvement should be better loop separation and fast GUI checks, not a Rust/Nix builder migration.
