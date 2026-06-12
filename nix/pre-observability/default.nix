{
  pkgs,
  guiArtifact,
  guiWaylandPerf,
}:
pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
  gui-wayland-perf-regression =
    pkgs.runCommand "klocc-gui-wayland-perf-check" {
      nativeBuildInputs = [guiWaylandPerf];
      KLOCC_GUI_PERF_WIDTH = "1920";
      KLOCC_GUI_PERF_HEIGHT = "1080";
      KLOCC_GUI_PERF_MAX_LAYOUT_MS = "150";
      KLOCC_GUI_PERF_MAX_PAINT_MS = "300";
    } ''
      work_dir=$(mktemp -d)
      if ! klocc-gui-wayland-perf ${guiArtifact}/self.sqlite "$out" >"$work_dir/gui-wayland-perf.log" 2>&1; then
        cat "$work_dir/gui-wayland-perf.log" >&2
        for log in weston.log wayland-info.txt gui.log perf-summary.txt; do
          if [[ -f "$out/$log" ]]; then
            printf '\n--- %s ---\n' "$log" >&2
            cat "$out/$log" >&2
          fi
        done
        exit 1
      fi
      cp ${guiArtifact}/scan.log ${guiArtifact}/check.log "$work_dir/gui-wayland-perf.log" "$out"/
    '';
}
