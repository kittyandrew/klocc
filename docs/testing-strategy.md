# Testing Strategy

`klocc` has three high-value regression surfaces: real Nix/source graph behavior, graph/read-model invariants, and native GUI behavior. These are represented as flake checks so CI and local validation can run the same entry points.

## Check Categories

### Real Project Scanner Checks

These checks scan real store paths and assert semantic SQLite invariants instead of exact fragile row counts.

- `checks.x86_64-linux.real-self-scan`: scans this flake's `klocc` package through its `.drv^out` installable. Covers dogfood source attribution, Rust/Cargo sources, schema validation, and check-runner behavior without requiring recursive Nix to evaluate a local flake URL from inside the sandbox.
- `checks.x86_64-linux.real-waybap-scan`: scans pinned `github:kittyandrew/waybap` through its realized `.drv^out` installable. Covers tiny runtime closure plus large build-source graph behavior without requiring a network flake fetch from inside the sandbox.
- `checks.x86_64-linux.real-hyprland-scan`: scans `nixpkgs#hyprland` through its realized `.drv^out` installable. Covers broad desktop closure behavior, generated derivation outputs, and mixed native dependency source classifications.

The shared runner is `scripts/check-real-project.sh`. It runs `klocc scan`, `klocc check`, and SQLite assertions for rollup arithmetic, treemap/read-model drift, source-kind expectations, source-health bounds, scan-health/table-count consistency, LOC-bearing source coverage, dependency edge counts, derivation/source link counts, total code LOC, and distinct source-kind/ecosystem coverage. Successful checks print top source-kind and ecosystem summaries so the result shape is visible in the build log.

These checks require `recursive-nix` because `klocc scan` shells out to Nix while running inside a Nix check derivation. Large real-project checks use pinned `.drv^out` targets instead of flake URL targets so recursive Nix can read derivation metadata without network access from the sandbox.

### Property And Invariant Tests

`checks.x86_64-linux.klocc-unit-property-tests` runs the Rust test suite for the scanner crate, including property tests for:

- Source rollup graph math over generated cyclic/shared dependency graphs.
- Artifact read-model projection invariants used by the GUI treemap.

These tests are cheap compared with real scanner checks and should run before the large real-project validations.

### GUI Snapshot Regression Checks

`checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston` and `checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland` are generated from `nix/snapshot/scenarios/treemap-drilldown.nix` and the environment backends in `nix/snapshot/environments/`. The scenario owns the regression thresholds; each VM backend enforces them after running the walkthrough.

The generated checks validate the shared dogfood artifact, capture every scenario step, assert screenshots are nonblank, verify multiple layout passes after drilldown/back navigation, and fail if logged layout or paint telemetry violates the scenario thresholds. The checks require minimum loaded source counts, rectangle counts, label counts, and zero sliver/tiny regressions so empty or degraded treemap screens cannot pass.

The screenshot artifact producer requires `recursive-nix` for the scanner phase.

The GUI check intentionally scans a pinned `.drv^out` installable, not a realized store output path and not a flake URL. A realized output path can lose derivation context and produce a visually empty treemap; a flake URL can require network-backed flake cache population inside the sandbox.

### GUI Pre-Observability Perf Check

`checks.x86_64-linux.gui-wayland-perf-regression` is owned by `nix/pre-observability/`. It consumes the same shared dogfood SQLite artifact as the snapshot checks, runs `klocc-gui` under Weston headless with `--fake-seat --refresh-rate=120000`, and writes `perf-summary.txt` from app timing telemetry instead of screenshots.

This check intentionally stays separate from snapshot methodology: it validates layout, paint, and response timing phases from `gui.log`, not visual output pixels.

## Suggested Local Order

Run the cheap/property layer first:

```sh
nix build .#checks.x86_64-linux.klocc-unit-property-tests
```

Then run the self scanner and GUI checks:

```sh
nix build .#checks.x86_64-linux.real-self-scan
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland
nix build .#checks.x86_64-linux.gui-wayland-perf-regression
```

Run large real-project checks one at a time to avoid Nix eval-cache contention and Nix store pressure:

```sh
nix build .#checks.x86_64-linux.real-waybap-scan
nix build .#checks.x86_64-linux.real-hyprland-scan
```

## Recursive Nix

Machines running the real scanner and GUI checks need recursive Nix enabled and the `recursive-nix` system feature available. Without it, the check derivation cannot safely run the scanner's internal Nix queries.
