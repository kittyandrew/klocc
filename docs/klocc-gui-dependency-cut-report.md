# klocc-gui Dependency Cut Report

Date: May 27, 2026

## Summary

The `klocc-gui` fresh release build is now materially smaller and faster after cutting avoidable dependencies while preserving the existing Wayland snapshot coverage.

| Metric | Before | After | Change |
| --- | ---: | ---: | ---: |
| Fresh `cargo build -p klocc-gui --release --timings` wall time | 273s | 196s | -77s, about 28% faster |
| Cargo target directory size | 1.7G | 1.4G | -0.3G |

The main local wins came from removing the scanner/CLI dependency graph from the GUI, unbundling SQLite, and turning off direct GPUI defaults.

## Changes Made

| Area | Change | Effect |
| --- | --- | --- |
| Artifact model | Added `crates/klocc-artifact` and moved SQLite artifact reading/model types there | Lets the GUI read artifacts without depending on the scanner/CLI crate |
| GUI dependency | Changed `klocc-gui` to depend on `klocc-artifact`, not `klocc` | Removes scanner, CLI, and LOC-counting dependencies from the GUI graph |
| SQLite | Changed `rusqlite` from bundled SQLite to system SQLite | Avoids compiling SQLite C source during the Rust build |
| Nix packaging | Added `pkg-config`/`sqlite` build inputs and SQLite runtime wrapping | Keeps Nix package/check builds linked against dynamic `libsqlite3` |
| GPUI | Set direct `gpui` dependency to `default-features = false` | Removes unused direct GPUI feature defaults |
| GPUI platform | Set `gpui_platform` to `default-features = false`, features `font-kit` and `wayland` | Removes direct X11 platform feature from the GUI package |

## Dependency Graph Result

`klocc-gui` no longer pulls these dependencies through its normal graph:

| Removed from GUI graph | Why it mattered |
| --- | --- |
| `klocc` | Scanner/CLI crate, too broad for a viewer-only GUI |
| `tokei` | Scanner LOC dependency, not needed by the artifact viewer |
| `clap` | CLI parsing dependency, not needed by the GUI viewer |
| `x11rb-protocol` | X11 protocol dependency removed by disabling direct X11 platform features |

SQLite remains in the GUI graph only through the viewer artifact path:

```text
klocc-gui -> klocc-artifact -> rusqlite -> libsqlite3-sys
```

The post-cut tree shows `libsqlite3-sys` with `pkg-config`, not bundled SQLite source compilation.

## Top Fresh Build Units

Before the cut, the top units included bundled SQLite and X11 protocol compilation:

| Rank | Unit | Time | Notable features |
| ---: | --- | ---: | --- |
| 1 | `libsqlite3-sys v0.35.0 build-script (run)` | 88.8s | `bundled`, `bundled_bindings`, `cc` |
| 2 | `naga v29.0.3` | 71.2s | shader translation stack |
| 3 | `gpui v0.2.2` | 61.7s | `default`, `font-kit`, `wayland`, `x11` |
| 4 | `image v0.25.10` | 59.2s | default image formats including AVIF/WebP |
| 5 | `wgpu-core v29.0.3` | 56.3s | WGPU core |
| 6 | `zbus v5.15.0` | 56.1s | portal/DBus stack |
| 7 | `x11rb-protocol v0.13.2` | 54.8s | X11 protocol features |
| 8 | `ravif v0.13.0` | 49.0s | AVIF encoder dependency |

After the cut, the largest remaining units are GPUI/WGPU/image/portal dependencies:

| Rank | Unit | Time | Notable features |
| ---: | --- | ---: | --- |
| 1 | `naga v29.0.3` | 65.6s | shader translation stack |
| 2 | `image v0.25.10` | 57.5s | default image formats including AVIF/WebP |
| 3 | `zbus v5.15.0` | 55.0s | portal/DBus stack |
| 4 | `gpui v0.2.2` | 54.9s | `bitflags`, `wayland` |
| 5 | `wgpu-core v29.0.3` | 48.4s | WGPU core |
| 6 | `ravif v0.13.0` | 45.2s | AVIF encoder dependency |

## Evidence

| Evidence | Location |
| --- | --- |
| Baseline summary | `/var/lib/factory/workspaces/klocc-gui-compile-profile-20260527-000018/build-summary.env` |
| Baseline Cargo timings | `/var/lib/factory/workspaces/klocc-gui-compile-profile-20260527-000018/target/cargo-timings/cargo-timing.html` |
| Post-cut summary | `/var/lib/factory/workspaces/klocc-gui-depcut-profile-20260527-020958/build-summary.env` |
| Post-cut Cargo timings | `/var/lib/factory/workspaces/klocc-gui-depcut-profile-20260527-020958/target/cargo-timings/cargo-timing.html` |
| Post-cut top units text | `/var/lib/factory/workspaces/klocc-gui-depcut-profile-20260527-020958/top-units.txt` |
| Post-cut dependency tree | `/tmp/opencode/klocc-gui-tree-after.txt` |
| Post-cut feature tree | `/tmp/opencode/klocc-gui-feature-tree-after.txt` |
| Post-cut duplicate dependency tree | `/tmp/opencode/klocc-gui-duplicates-after.txt` |

## Verification Run

These checks passed after the dependency cuts:

```sh
nix develop -c cargo check -p klocc-gui
nix develop -c cargo check -p klocc
nix develop -c cargo check -p klocc-gui --all-targets
nix develop -c cargo test -p klocc-artifact -p klocc-gui --lib
nix build .#klocc-gui
nix build .#klocc
nix build .#checks.x86_64-linux.klocc-unit-property-tests
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland
nix build .#checks.x86_64-linux.gui-snapshot-metric-runtime-drilldown-weston
nix build .#checks.x86_64-linux.gui-snapshot-metric-runtime-drilldown-hyprland
nix run nixpkgs#alejandra -- --check flake.nix nix
nix run nixpkgs#deadnix -- --fail flake.nix nix
git diff --check
```

## Next Frontier

The easy app-level dependency cuts are done. The remaining top units mostly come from the GPUI/WGPU/image/portal stack, not from this app's scanner dependencies.

Follow-up source inspection at the pinned GPUI/WGPU revisions found no safe local-only switch for the largest remaining crates:

| Area | Source checked | Finding |
| --- | --- | --- |
| GPUI image formats | `/tmp/opencode/zed-gpui-560f/crates/gpui/Cargo.toml`, `/tmp/opencode/zed-gpui-560f/crates/gpui/src/platform.rs`, `/tmp/opencode/zed-gpui-560f/crates/gpui/src/elements/img.rs` | `gpui` depends on workspace `image` directly, and image loading supports PNG, JPEG, WebP, GIF, BMP, TIFF, ICO, PNM, and SVG. Narrowing this would require a GPUI feature change or fork. |
| Linux portals/DBus | `/tmp/opencode/zed-gpui-560f/crates/gpui_linux/Cargo.toml`, `/tmp/opencode/zed-gpui-560f/crates/gpui_linux/src/linux/platform.rs`, `crates/klocc-gui/src/main.rs` | `gpui_linux` uses `ashpd` for file picker/open URI/settings. `klocc-gui` currently calls `cx.prompt_for_paths`, so dropping the portal stack would require removing or replacing the native file picker. |
| WGPU/Naga features | `/tmp/opencode/zed-gpui-560f/crates/gpui_wgpu/Cargo.toml`, `/tmp/opencode/zed-wgpu/wgpu/Cargo.toml`, `/tmp/opencode/zed-wgpu/wgpu-core/Cargo.toml`, `/tmp/opencode/zed-wgpu/wgpu-hal/Cargo.toml` | `gpui_wgpu` depends on workspace `wgpu`; the pinned WGPU defaults enable native backend/shader features. Narrowing Vulkan/GLES/RenderDoc/Naga output support requires changing the Zed/GPUI workspace dependency or patching/forking WGPU usage. |

Plausible next investigations:

1. Check whether GPUI can disable unused `image` formats without forking.
2. Check whether GPUI's portal/DBus path is needed for this viewer in the current Linux build.
3. Check whether WGPU/Naga feature selection can be narrowed safely in Zed/GPUI dependencies.
4. Only consider a GPUI/WGPU fork after source-level feature research proves the upstream graph has no supported knobs.

Do not treat those as local cleanup. They are upstream dependency feature-shaping work and should be measured in a branch before adopting.
