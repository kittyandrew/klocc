# Treemap Layout Regression Plan

## Problem

The Weston/Xvfb screenshot check can now drive and capture all four GUI states, but it fails the existing paint quality gate with source slivers in a drilled view:

```text
klocc-gui-wayland-e2e: expected slivers <= 0, got 2
```

The reproduced shape is a drilled `root Some(615)` layout with one dominant `derivation-env-src`, one medium `fixed-output-source`, and a very small `nix-input-src` aggregate. The grouped source-kind layout preserves area, but it forces medium groups into tall narrow columns and leaves the tiny aggregate as a short strip.

## Constraints

- Do not loosen the screenshot quality gate to hide bad layout.
- Do not inflate tiny aggregate area; area must remain proportional to the selected metric.
- Tiny aggregate cells are real layout cells and clickable, so they must satisfy the same minimum rendered side policy as source cells.
- Prefer source-kind grouping when it produces readable geometry.
- Fall back only when grouping creates unreadable source slivers or tiny aggregate strips.

## Regression Tests

Add deterministic tests for the exact failing drilled shape:

- `regression_drilled_layout_does_not_emit_source_slivers`
- `regression_drilled_layout_does_not_emit_tiny_aggregate_strips`

Add property-based coverage so future changes do not only satisfy the fixture:

- Randomized layouts should not emit unreadable source rects.
- Randomized layouts should not emit tiny aggregate strips.
- Existing area-scale assertions stay in place to prevent fake layout-weight inflation.

## Fix Direction

Use the current source-kind grouped layout as the first attempt. If its output contains unreadable source slivers or tiny aggregate strips, switch that view to an adaptive flat layout:

- Keep source-kind colors on each rect.
- Merge tiny or unreadable source rects into one tiny aggregate per group.
- Relayout iteratively until source rects are readable and tiny aggregate cells are not strips, or until no further merging is possible without violating area proportionality.
- If a tiny group remains physically too small, fold it into a broader small-groups aggregate instead of inflating its area.

## Verification

- First verify the new regressions fail on current code for the expected reasons.
- Then implement the layout change and run `cargo test -p klocc-gui layout::tests --locked`.
- Finally rerun the Weston/Xvfb and Hyprland VM screenshot checks.

## Implementation Notes

Implemented the fix by extending the existing tiny-merge path rather than introducing a second layout engine:

- Top-level source-kind groups that would render as tiny or unreadable are folded into a single `small-source-kinds` aggregate.
- Per-group source entries that would render as tiny or unreadable are folded into that group's tiny aggregate bucket before final rect emission.
- Aggregate accounting now preserves source counts when aggregates are combined, so dependency conservation still counts underlying source ids rather than intermediate buckets.
- The existing area-scale guard remains active; the fix removes bad geometry by merging cells, not by inflating tiny cell area.

Regression coverage now includes:

- deterministic drilled-view fixture matching the Weston VM failure shape.
- deterministic assertion for tiny aggregate strips in that same fixture.
- targeted proptest over the same high-ratio drilled-view family.

Verified after implementation:

- `nix build .#klocc-gui --no-link`
- `nix develop -c cargo fmt --check`
- `nix develop -c cargo clippy -p klocc-gui --all-targets --locked -- -D warnings`
- `bash -n scripts/gui-wayland-lib.sh nix/pre-observability/gui-wayland-perf-check.sh`
- `nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston --out-link /tmp/opencode/klocc-treemap-drilldown-weston-result`
- `nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland --out-link /tmp/opencode/klocc-treemap-drilldown-hyprland-result`
