# GUI Screenshot Input Backends

## Weston/Xvfb First-Hover Quirk

The Weston/Xvfb screenshot backend drives input through the X11 host window that contains Weston:

- `nix/snapshot/environments/weston.nix` calls `xdotool mousemove --window "$weston_window" ...`.
- `xdotool` implements `mousemove --window` by translating the target to root coordinates and calling `XWarpPointer`.
- Weston's X11 backend handles an X11 enter separately from motion. `XCB_ENTER_NOTIFY` calls `notify_pointer_focus(...)`; `XCB_MOTION_NOTIFY` calls `notify_motion(...)`.
- GPUI's Wayland client handles `wl_pointer.enter` by storing `mouse_location` and calling `window.set_hovered(true)`, but it only emits `PlatformInput::MouseMove` for `wl_pointer.motion`.
- `klocc-gui` updates treemap hover only from `.on_mouse_move(...)`, so a pointer enter alone can show the compositor cursor over a rectangle without repainting the app hover highlight.

References checked on May 25, 2026:

- `nix/snapshot/environments/weston.nix`: `pointer_move_app()` uses `xdotool mousemove --window`.
- `/nix/store/4h9y7gqldv4illvc65xv5xn9rqk9snil-source/cmd_mousemove.c`: `_mousemove()` calls `xdo_move_mouse_relative_to_window()` for `--window` moves.
- `/nix/store/4h9y7gqldv4illvc65xv5xn9rqk9snil-source/xdo.c`: `xdo_move_mouse_relative_to_window()` calls `xdo_move_mouse()`, which uses `XWarpPointer`.
- `/nix/store/45aypy9rxqs6ywl8419zhy2r342ngk91-source/libweston/backend-x11/x11.c`: `x11_backend_deliver_enter_event()` calls `notify_pointer_focus(...)`; `x11_backend_deliver_motion_event()` calls `notify_motion(...)`.
- `/tmp/opencode/zed-gpui/crates/gpui_linux/src/linux/wayland/client.rs`: `wl_pointer::Event::Enter` calls `window.set_hovered(true)`; `wl_pointer::Event::Motion` emits `PlatformInput::MouseMove`.
- `crates/klocc-gui/src/main.rs`: treemap hover is updated in `.on_mouse_move(...)`, and paint highlighting is based on `self.hovered`.

### Experiments

The original symptom was specific to W-1: the cursor was visibly over the largest root rectangle, but the rectangle was not hover-highlighted. W-2, W-3, and W-4 highlighted because they all happened after a successful treemap click.

Failed hypotheses and workarounds:

- App-relative coordinates were wrong. Rejected because the same coordinate clicked successfully and drilled into `root Some(580)`.
- Waiting for first layout telemetry was enough. Rejected because `rect manifest end` is emitted during layout, before a rendered frame necessarily has active mouse listeners.
- Waiting for first paint plus app-origin pointer priming was enough. Rejected by pixels: W-1 still sampled unhovered `srgb(239,159,118)`.
- Two-step absolute pointer motion was enough. Rejected by pixels and logs.
- Relative motion from app origin was enough. Rejected by pixels and logs.
- A generic inert app focus click was enough. Rejected by pixels and logs.

Successful experiment:

- Before running the scenario, perform a real treemap canvas click at the same root source used by the first scenario step, then right-click back to root.
- After that, W-1 hover-highlighted correctly and matched Hyprland's sampled hover color: `srgb(244,191,164)` at both sampled points.
- The Weston log gained a root-view hover response before the first scenario drilldown, followed by the normal `root Some(580)` layout from the scenario click.

Interpretation:

- The first cold-start Weston/X11 hover path is unreliable for GPUI's treemap canvas, even when the visible cursor is positioned correctly.
- A generic focus click outside the treemap does not fix it.
- A real prior canvas interaction does fix it, so the missing condition is specific to GPUI/Wayland canvas mouse handling after the canvas has received a real interaction, not simple window activation.
- The canvas-prime step is currently evidence and a backend workaround, not an ideal final abstraction. A cleaner long-term fix would use Wayland-native input for the Weston backend if Weston exposes the needed virtual pointer protocol, or a narrowly gated app-side test hook if product-level visual parity is more important than keeping input entirely compositor-driven.
