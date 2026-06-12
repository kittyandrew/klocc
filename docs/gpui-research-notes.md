# GPUI Research Notes

Date: May 21, 2026

GPUI source inspected at `/tmp/opencode/zed-gpui` from `https://github.com/zed-industries/zed.git`.

## Files Checked

- `/tmp/opencode/zed-gpui/crates/gpui/README.md`
- `/tmp/opencode/zed-gpui/crates/gpui/docs/contexts.md`
- `/tmp/opencode/zed-gpui/crates/gpui/examples/hello_world.rs`
- `/tmp/opencode/zed-gpui/crates/gpui/examples/painting.rs`
- `/tmp/opencode/zed-gpui/crates/gpui/examples/input.rs`
- `/tmp/opencode/zed-gpui/crates/gpui/src/app.rs`
- `/tmp/opencode/zed-gpui/crates/gpui/src/platform.rs`
- `/tmp/opencode/zed-gpui/crates/workspace/src/workspace.rs`

## Implementation Implications

- App startup should follow `gpui_platform::application().run(|cx: &mut App| { ... })` and create the root window with `cx.open_window(WindowOptions { ... }, |window, cx| cx.new(|cx| RootView::new(window, cx)))`.
- The UI should be a root `Entity` implementing `Render`; mutations that should redraw call `cx.notify()`.
- The file picker should use `App::prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(...) })` and handle the asynchronous result.
- The treemap should not be represented as thousands of `div()` children. Use `canvas(...)` and paint rectangles through window paint primitives, following `examples/painting.rs`.
- Hit testing should be app-owned data over precomputed treemap rectangles. Mouse move updates hovered rectangle metadata; left click changes the current treemap root; right click navigates back.
- Startup/path entry chrome can use normal `div()`/input-style views. The large treemap body should be custom-painted.

## Current UI Requirements From Interview

- Viewer only: open existing SQLite artifacts; do not run scans from the native app.
- First screen has a manual file path field and a system file-picker action.
- After selecting a DB, transition directly to a full treemap screen.
- Default area metric is source code LOC.
- Default color is runtime-linked versus build-time-only.
- Top row has mode/metric/completeness selectors.
- Left click drills into direct dependencies.
- Right click goes back.
- Left side hover panel shows details for the rectangle under the mouse.
- It must target package scans and larger NixOS-scale scans from the start.
- Strict current schema only; no migrations yet.
- Catppuccin Frappe visual style.

## Verification Notes

- `KLOCC_GUI_EXERCISE=1 klocc-gui <artifact.sqlite>` loads an artifact and exercises mode layouts, cache hits, hit testing, and drilldown without opening a GPUI window. This is for CI/headless verification only; the app remains a viewer and does not run scans.
- Generated `gui-snapshot-<scenario>-<environment>` checks run full server-side visual Wayland E2E checks in NixOS VMs. They launch the GPUI app against the shared SQLite artifact, drive the scenario through the environment backend, enforce scenario-owned regression assertions, and capture PNG screenshots plus logs in the output directory.
- `klocc-gui-wayland-perf <artifact.sqlite> [output-dir]` lives under `nix/pre-observability/` and runs the four-state performance walkthrough under Weston headless with `--fake-seat --refresh-rate=120000`, driven by `KLOCC_GUI_INTERNAL_PERF=1`. This is the default perf topology because it keeps the app as a Wayland client and avoids X11 screenshot/input timing.
- Screenshot E2E stays on Weston X11 backend because headless has no X output window for `xdotool`/`xwd`. Perf and screenshots are intentionally separate.

## 120Hz Headless Perf Decision

- Date: May 24, 2026.
- Decision: default the headless perf runner to 120Hz, not 240Hz.
- Evidence: repeated 120Hz/240Hz runs showed 240Hz did not reliably reduce total response and produced worse outliers in `before_layout` and response paint.
- Weston source checked: `/tmp/opencode/weston-src-link/libweston/backend-headless/headless.c` and `/tmp/opencode/weston-src-link/libweston/compositor.c`.
- `headless.c` stores `--refresh-rate` in `mode.refresh`, but comments note Wayland event-source timeout granularity is on the order of milliseconds.
- `compositor.c::weston_output_arm_frame_timer` rounds frame timer delays through `wl_event_source_timer_update(... DIV_ROUND_UP(delay_nsec, 1000000))`, so sub-millisecond frame precision is unavailable.
- `compositor.c::weston_output_repaint_msec` clips the repaint window to at least 1ms shorter than the refresh period. At 240Hz the frame period is about 4.17ms, leaving very little tolerance for Linux scheduling jitter and app paint variance.
- Conclusion: 120Hz is the stable high-refresh simulation for this harness; 240Hz mostly measures software timer quantization and scheduler jitter rather than useful app responsiveness.

## Native Capture Cursor Injection

- Date: May 24, 2026.
- Finding: Weston headless native screenshots via `weston-screenshooter` work with `--debug` and `weston_capture_v1`; cursor correctness depends on driving the real compositor pointer rather than mutating app state internally.
- Source checked: `/tmp/opencode/weston-src-link/protocol/weston-output-capture.xml`, `/tmp/opencode/weston-src-link/clients/screenshot.c`, `/tmp/opencode/weston-src-link/libweston/input.c`, `/tmp/opencode/weston-src-link/libweston/backend-headless/headless.c`, `/tmp/opencode/weston-src-link/tests/weston-test.c`, and `/tmp/opencode/weston-src-link/protocol/weston-test.xml`.
- Cursor source detail: Weston headless creates a fake pointer at `(100,100)`. GPUI sets a Wayland cursor surface when a client has pointer focus. The app-internal perf/screenshot driver mutates app state directly; it does not inject real Wayland pointer motion/button events, so native capture can show a stale compositor cursor even while app hover state changes.
- Validated path: Weston’s private `weston_test` protocol can move the compositor pointer and send buttons. The installed package does not ship `weston-test.so`, and the stock built `tests/test-plugin.so` assumes Weston test-harness private data and segfaults when used standalone. A local null-safe build of that module exposed `weston_test`; a tiny client moved the real pointer from `(100,100)` to `(240,180)` and received the corresponding `pointer_position` event.
- Tradeoff: the real-pointer path is more honest than an in-app debug marker, but it relies on a private, explicitly non-installed Weston testing protocol plus a locally built/patched module. Use it for validation tooling only, not product runtime behavior.
- Fallback if carrying the private module is not acceptable: paint an env-only in-app debug cursor/hover marker at the scripted target point so native capture includes it as app content. Do not silently rely on the stale headless compositor cursor.
