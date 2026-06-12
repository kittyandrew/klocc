# GUI Screenshot Scenario Plan

## Goal

Make GUI screenshot validation reusable across compositor environments and walkthroughs from one readable scenario file.

The scenario file owns product-level walkthrough data and optional regression assertions:

- scenario labels and descriptions.
- screenshot step labels and descriptions.
- fixed app-relative action coordinates for each walkthrough step.
- artifact, telemetry, and image invariants that define whether the scenario is a regression gate.

Environment code owns only backend mechanics: compositor startup, pointer movement/clicking, screenshot capture, and VM readiness checks.

## Current Constraint

Hyprland and Weston/Xvfb are not interchangeable implementations of the same low-level harness.

- Hyprland needs a VM DRM/render node, virtio-gpu, Hyprland config, `kms_swrast` evidence, `hyprctl`, `wlrctl`, and `grim`.
- Weston/Xvfb needs `Xvfb`, Weston X11 backend, `xdotool`, `xwd`, and a Weston X11 output window.

Those backend details stay explicit. The shared layer starts at fixed app-relative actions and review metadata.

## Scenario Contract

`nix/snapshot/scenarios/treemap-drilldown.nix` is the single source for the current walkthrough. It is a plain Nix attrset so NixOS VM checks import it directly and non-Nix tools can read it through `nix eval --json --file`. It contains:

- `id`, `title`, `label`, and `description`.
- `screen.width`, `screen.height`, `app.width`, and `app.height`.
- `artifacts.perStepExtensions` and `artifacts.common`.
- `assertions`: optional artifact, telemetry, and image thresholds enforced by generated VM checks.
- `steps`: ordered screenshot states for review rendering and execution.
- `steps[].actions`: fixed-coordinate walkthrough instructions that run before the implicit capture for that step.

Coordinates are app-relative, with the app pinned to `1760x940` inside a `1920x1080` output. Backends translate them into their own input systems:

- Weston/Xvfb: `xdotool --window <weston-output> app_left+x app_top+y`.
- Hyprland: `hyprctl dispatch movecursor app_left+x app_top+y` plus `wlrctl pointer click`.

Backends prime the cursor at app-relative origin before running scenario actions so the first scenario `move` is delivered as motion rather than only pointer enter. See `docs/gui-screenshot-input-backends.md`.

Step output paths are inferred from order, ID, and `artifacts.perStepExtensions`. For example, the first step with ID `current` and extensions `png`/`stats` writes `01-current.png` and `01-current.stats`. VM checks copy those inferred step artifacts and `artifacts.common`.

Common artifacts use standard names across environments: `klocc-gui.log` for app output and `environment.log` for all backend/harness evidence. `environment.log` is sectioned and greppable with headers such as `[scenario]`, `[backend]`, `[walkthrough.stats]`, `[wayland-info]`, `[compositor]`, `[drm]`, `[xvfb]`, and `[xwininfo]`. Do not add per-environment copied sidecar logs unless the data is too large or too structured to remain useful inside `environment.log`.

Environment result paths are inferred from scenario ID and environment ID. For example, scenario `treemap-drilldown` defaults to `/tmp/opencode/klocc-treemap-drilldown-hyprland-result` and `/tmp/opencode/klocc-treemap-drilldown-weston-result`.

Snapshot VM checks are generated from every scenario file in `nix/snapshot/scenarios/` across every environment backend in `nix/snapshot/environments/`. Check names use `gui-snapshot-<scenario-id>-<environment-id>`, for example `gui-snapshot-treemap-drilldown-hyprland` and `gui-snapshot-treemap-drilldown-weston`.

## Regression Contract

Regression checks are not a separate scenario runner. A regression is a scenario with `assertions` strong enough to fail on known bad output: undersized dogfood artifacts, missing GUI telemetry, zero-rectangle layouts, too few labels, unexpected slivers/tiny rectangles, or blank screenshots.

Generated snapshot VM checks enforce those scenario assertions after running the walkthrough. This keeps the matrix as `scenario x environment = check` and avoids maintaining a parallel Bash E2E implementation for the same walkthrough.

The legacy `gui-wayland-regression` shell runner has been retired. Its assertions live in `nix/snapshot/scenarios/*.nix` and are enforced by `nix/snapshot/environments/*.nix`. Host-runnable manual Weston/Xvfb testing should use a distinct manual/debug tool if it remains useful; it should not be the implementation of flake snapshot checks.

## Review Contract

The review generator reads the same scenario file plus environment output directories. It should not depend on per-run generated review metadata.

This keeps new walkthrough review pages simple: add steps/actions to one scenario file, run both environment checks, and render the share page from that file.

## Non-Goals

- Do not build a generic compositor VM abstraction.
- Do not merge Hyprland and Weston startup paths.
- Do not make the review script run checks or upload by itself.
- Do not rediscover dynamic target coordinates during every screenshot run unless a future scenario needs it.

## Future Extension

For menus or filters, add fixed `move`/`click`/`capture` actions first. If coordinates become unstable, add app-side control telemetry later and update the scenario once from that telemetry.
