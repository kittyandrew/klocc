# Snapshot Environment Extraction Plan

## Goal

Reduce duplicated logic between `nix/snapshot/environments/weston.nix` and `nix/snapshot/environments/hyprland.nix` without hiding backend-specific compositor, input, or capture behavior.

## Scope

- Extract shared Nix-derived scenario metadata.
- Extract shared Python assertion helpers for artifact, telemetry, and image stats.
- Extract shared Python walkthrough montage construction.
- Move review environment labels/order out of Python and into the snapshot environment registry.

## Non-Goals

- Do not abstract backend startup/readiness.
- Do not abstract Weston `xdotool`/`xwd` input/capture with Hyprland `hyprctl`/`wlrctl`/`grim` input/capture.
- Do not merge pre-observability perf with snapshot environments.

## Progress

- [x] Add shared Nix metadata helper.
- [x] Move duplicated artifact/telemetry/image assertions into a Python helper.
- [x] Move duplicated four-image walkthrough montage logic into the Python helper.
- [x] Move review environment labels/order into `nix/snapshot/environments/default.nix`.
- [x] Verify generated Weston and Hyprland snapshot checks still pass.

## Verification Plan

- [x] `nix run nixpkgs#alejandra -- --check flake.nix nix`
- [x] `nix run nixpkgs#deadnix -- --fail flake.nix nix`
- [x] `python -m py_compile nix/snapshot/assertions.py`
- [x] `python -m py_compile nix/shared/gui-review/wayland-gui-review.py`
- [x] `nix eval --json --file nix/snapshot/environments/default.nix review`
- [x] `nix eval --json .#checks.x86_64-linux --apply builtins.attrNames`
- [x] `nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston --out-link /tmp/opencode/klocc-treemap-drilldown-weston-result`
- [x] `nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland --out-link /tmp/opencode/klocc-treemap-drilldown-hyprland-result`
