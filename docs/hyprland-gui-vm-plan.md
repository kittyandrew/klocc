# Hyprland GUI VM Check Plan

## Goal

Run `klocc-gui` in a reproducible flake check under Hyprland without depending on a physical host GPU.

## Chosen Environment

Use a NixOS test VM with QEMU `virtio-gpu-pci`:

- The host still does not need `/dev/dri` or a real GPU.
- The guest gets `/dev/dri/card0` and `/dev/dri/renderD128` from virtio-gpu.
- Hyprland/Aquamarine gets the DRM/GBM allocator it requires.
- Mesa falls back to software rendering through `kms_swrast`/llvmpipe.

## Why Not Pure Headless

Hyprland 0.55.1 creates Aquamarine backends in this order: mandatory headless, optional DRM, fallback Wayland. Aquamarine's headless backend does not provide a DRM FD, and `CBackend::start()` fails without an allocator. That makes pure Hyprland headless without any DRM/render node fail with `Cannot open backend: no allocator available`.

Nested Hyprland under Sway headless also fails for this version because Aquamarine's Wayland backend requires `zwp_linux_dmabuf_v1`; Sway's pure headless backend did not expose that protocol in the tested setup.

## Check Shape

The flake check should:

1. Build a SQLite artifact outside the VM using `klocc scan` and `klocc check`.
2. Boot a NixOS VM with `-vga none -device virtio-gpu-pci`.
3. Start Hyprland as the test user with a minimal config.
4. Assert `/dev/dri/card0` and `/dev/dri/renderD128` exist.
5. Assert Hyprland logs include `kms_swrast` to prove software rendering.
6. Assert Hyprland exposes `zwlr_screencopy_manager_v1`, `zwlr_virtual_pointer_manager_v1`, and `zwp_linux_dmabuf_v1`.
7. Launch `klocc-gui` against the artifact.
8. Wait for load and paint-quality telemetry.
9. Move the compositor pointer with `wlrctl`.
10. Capture a full screenshot with `grim -c`.
11. Assert the PNG is present, the expected size, and nonblank.
12. Copy the screenshot and logs out of the VM test result.

## Evidence Checked

- Hyprland source: `src/Compositor.cpp` backend setup.
- Aquamarine source: `src/backend/Backend.cpp`, `src/backend/Headless.cpp`, `src/backend/Wayland.cpp`.
- Upstream Hyprland NixOS test: `nix/tests/default.nix` uses `-vga none -device virtio-gpu-pci`.
- Local smoke result: Hyprland started in the VM, `grim -c` captured `640x480`, and logs showed `falling back to kms_swrast`.
