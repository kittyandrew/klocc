# Wayland GUI E2E In Server Environments

- Use this when validating the native GPUI viewer in a headless/server workspace.
- Do not conclude "GUI cannot run here" after plain `Xvfb` fails; GPUI/wgpu needs a usable GL/Vulkan stack.
- The visual screenshot topology is: `Xvfb` -> Weston X11 backend -> Wayland client app -> Mesa Lavapipe Vulkan.
- The perf topology is: Weston headless backend with `--fake-seat --refresh-rate=120000` -> Wayland client app -> Mesa Lavapipe Vulkan, driven by the app-internal `KLOCC_GUI_INTERNAL_PERF=1` walkthrough.
- Do not use Weston headless for screenshot review: it has no X11 output window for `xdotool`/`xwd`; screenshot E2E stays on the X11 backend unless a Wayland-native capture path is added.
- Do not raise the default headless perf refresh rate to 240Hz. Weston headless uses millisecond-granularity software timers; at 240Hz the 4.17ms frame period makes timer quantization and scheduler jitter dominate. Repeated samples showed worse outliers than 120Hz.
- Native headless screenshots with `weston-screenshooter` can show a compositor cursor, but the cursor is stale unless the real Wayland pointer is moved. The internal driver mutates app state directly instead of injecting Wayland pointer events. If cursor visibility matters, prefer real pointer injection via a validation-only Weston `weston_test` module/client; use an env-only in-app debug cursor marker only as a documented fallback.
- Plain `Xvfb` is not enough for wgpu if GLX/Vulkan drivers are absent.
- Do not export/use `WAYLAND_SOCKET` as an internal variable; libwayland treats it as an inherited socket fd. Use `KLOCC_WAYLAND_DISPLAY_NAME` or similar.

## Runners

- Prefer generated snapshot checks over hand-rolled screenshot commands:

```sh
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-weston --out-link /tmp/opencode/klocc-treemap-drilldown-weston-result
nix build .#checks.x86_64-linux.gui-snapshot-treemap-drilldown-hyprland --out-link /tmp/opencode/klocc-treemap-drilldown-hyprland-result
```

- Screenshot regression assertions live in the scenario record and are enforced by the generated VM environment checks. Do not reintroduce a separate `gui-wayland-e2e` host runner as a flake check.

- Use the separate perf runner for regression gates; it drives the same four-state walkthrough at 120Hz headless Wayland, but does not capture or compose screenshots:

```sh
nix run .#gui-wayland-perf -- /tmp/klocc-self.sqlite /tmp/opencode/klocc-wayland-perf
```

- The perf runner enables `KLOCC_GUI_PROFILE=1`, so `gui.log` includes `paint profile` lines and `perf-summary.txt` includes phase timings for response, rect fills, label fitting, text painting, and text shaping.
- The perf flake check is owned by `nix/pre-observability/` and consumes the shared GUI SQLite artifact produced for snapshot checks; do not add a second inline scan path for perf.
- Perf gates should treat `response_total` as frame-paced latency and app work separately. At 120Hz headless, expected frame wait is roughly 6-9ms; app-controlled response work should stay under the stricter `response_layout`, `response_paint`, and `response_app` gates.
- For exploratory live validation, use the persistent controller:

```sh
nix run .#gui-wayland-live -- start /tmp/klocc-self.sqlite
nix run .#gui-wayland-live -- capture live-initial
nix run .#gui-wayland-live -- click <x> <y> [button]
nix run .#gui-wayland-live -- capture after-click
nix run .#gui-wayland-live -- stop
```

- Live mode keeps Xvfb, Weston, and the GPUI app running between commands so the agent can inspect screenshots, decide the next click, and iterate on actual state.

## Screenshot Runner Requirements

- Start `xvfb-run -a -s "-screen 0 <scenario width>x<scenario height>x24"`.
- Start Weston with `--backend=x11 --renderer=pixman --socket=<name> --width=<scenario width> --height=<scenario height>`.
- Set `XDG_RUNTIME_DIR` to a fresh `0700` temp dir.
- Set `VK_DRIVER_FILES` to Mesa Lavapipe `lvp_icd.*.json`.
- Run the GUI with `env -u DISPLAY WAYLAND_DISPLAY=<name> ...` so it is a Wayland client.
- Wait for the socket, then briefly wait/poll `wayland-info`; the socket can exist before clients are accepted.
- Require `wayland-info` to show `wl_seat`, `xdg_wm_base`, and `wl_output`.
- Use `xdotool` against the Weston X11 output window to drive pointer input.
- Use `xwd` + ImageMagick to capture PNGs and check non-blank image stats.
- Capture the four-step walkthrough, not just one final screenshot: root view, after clicking the largest top-left source rect, after clicking the largest source rect in that second-layer view, and after returning to root and clicking a gray tiny aggregate rect.
- Drive the screenshot walkthrough from `nix/snapshot/scenarios/treemap-drilldown.nix`; coordinates in that file are app-relative and were derived from the stable GUI rect manifests for the pinned artifact/window size.
- Keep screenshot generation separate from performance regression testing. The perf runner should assert `gui.log` timing telemetry and write `perf-summary.txt`; it should not depend on PNG capture or ImageMagick composition.

## Visual Review HTML

- Prefer a self-contained HTML review page over a composed PNG. Keep each screenshot embedded at full resolution, with clickable grid cards and individual full-image views.
- Use `nix/snapshot/scenarios/treemap-drilldown.nix` as the single source for scenario labels, screenshot step labels, and app-relative walkthrough coordinates.
- Use `nix/shared/gui-review/wayland-gui-review-template.html` as the page template. Do not inline the full HTML template in this rule.
- Use `nix/shared/gui-review/wayland-gui-review.py` to load the scenario JSON and generate the review controls/cards/single-image views dynamically from the environment result directories.
- The default selected view is a comparison grid with rows ordered by scenario states and columns ordered by the scenario environment order.
- For the default current review, Hyprland is the left column and Weston/Xvfb is the right column.
- The top control row includes the scenario compare view plus one grid view per environment.
- Directly below those buttons, include compact individual-image buttons generated from scenario environment shorts and state indexes, such as `H-1` and `W-1`.
- Clicking any grid image must open the same individual full-image view as its corresponding compact button. Keyboard activation with `Enter` or `Space` should work too.
- Grid margins should be minimal: small body padding, small card border radii, and grid gaps around `6px` so the screenshots retain as much screen area as possible.
- The 2x4 comparison and 2x2 environment grids must fit inside the visible viewport without requiring page scrolling; use contained images inside fixed row/column grid cells rather than letting image natural height determine page height.
- Individual-image view should use the available viewport, keep the screenshot aspect ratio, and use `object-fit: contain` instead of cropping.
- Clicking the screenshot in an individual-image view should toggle an explicit zoomed mode with scrollbars. The zoom must anchor around the clicked image point, not jump to the top-left. Do not only switch to native image size; that can look unchanged on wide displays.
- Use `nix/shared/gui-review/wayland-gui-review.py` to generate the page. It loads the scenario file and injects screenshots from the environment output directories.

```sh
nix shell nixpkgs#python3 -c python3 nix/shared/gui-review/wayland-gui-review.py
```

- The script only renders the full HTML and prints the output path. It must not run `kshare` internally.
- Upload new review pages only when no relevant URL exists, using the path printed by the script:

```sh
html=$(nix shell nixpkgs#python3 -c python3 nix/shared/gui-review/wayland-gui-review.py)
kshare "$html" --ttl 7d
```

- Replace an existing review URL whenever the slug is known. For example, if the current session already shared `https://s.ndrew.me/s/PYy6yXBR`, use:

```sh
html=$(nix shell nixpkgs#python3 -c python3 nix/shared/gui-review/wayland-gui-review.py)
kshare replace PYy6yXBR "$html" --ttl 7d
```

- When reporting the review link, include the URL and state whether it was replaced or newly created.

## Treemap Visual Constraints

- Treat selected metric area as invariant across source rects and gray tiny aggregate rects; do not make low-value gray aggregates readable by inflating their layout weight.
- Treemap labels should be dense and direct: paint `<name>` on the first line and `<loc>` directly below it, over the rectangle itself, without a darker badge/panel, and avoid showing source-kind text like `drv source` in the rectangle label.
- Preserve readability with WCAG-style contrast selection, opaque text, semibold/medium font weights, and whole-pixel text origins. Avoid fractional-offset outline/glow passes for small text because they blur on the pastel treemap colors.
- Show an interactive breadcrumb path above the treemap, e.g. `klocc -> rustc -> libxyz`; every prior crumb should navigate back to that view, and the current crumb should be visually current but not clickable.
- Treat placeholder source names like `source`/`src` as bugs to resolve from available context; they are not acceptable visible treemap labels when better metadata exists.

## Expected Artifacts

- `klocc-gui.log`
- `environment.log`
- `<step-index>-<step-id>.png` for every scenario step, such as `01-current.png`.
- `walkthrough.png`
- `perf-summary.txt` for perf runs
- `*.stats`

## Failure Triage

- `klocc-gui.log`: GPUI load/layout/panic output.
- `environment.log`: standardized scenario/backend summary plus compositor, Wayland, DRM/Xvfb/X11 diagnostics under greppable section headers.

## Non-Pixel Exercise Path

- Keep this separate from the headless logic exercise path:

```sh
KLOCC_GUI_EXERCISE=1 klocc-gui /tmp/klocc-self.sqlite
```

- The exercise path validates DB/layout/hit-test/drilldown without pixels; the Wayland E2E runner validates real launch, rendered pixels, and input delivery.
- Live validation is screenshot-driven exploratory testing, not a fixed coordinate script; capture, inspect, click, recapture, and fix behavior issues discovered this way.
