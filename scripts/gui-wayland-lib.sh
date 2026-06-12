#!/usr/bin/env bash

klocc_find_gui_bin() {
  if [[ -n "${KLOCC_GUI_BIN:-}" ]]; then
    printf '%s\n' "$KLOCC_GUI_BIN"
  elif [[ -x ./result/bin/klocc-gui ]]; then
    printf '%s\n' ./result/bin/klocc-gui
  elif [[ -x ./target/debug/klocc-gui ]]; then
    printf '%s\n' ./target/debug/klocc-gui
  else
    return 1
  fi
}

klocc_find_lavapipe_icd() {
  if [[ -n "${VK_DRIVER_FILES:-}" ]]; then
    printf '%s\n' "$VK_DRIVER_FILES"
    return 0
  fi
  if [[ -n "${KLOCC_MESA_DIR:-}" ]]; then
    for candidate in "$KLOCC_MESA_DIR"/share/vulkan/icd.d/lvp_icd.*.json; do
      if [[ -f "$candidate" ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
    done
  fi
  return 1
}

klocc_wait_for_wayland_socket() {
  local runtime_dir=$1
  local wayland_name=$2
  for _ in $(seq 1 100); do
    if [[ -S "$runtime_dir/$wayland_name" ]]; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

klocc_wait_for_wayland_info() {
  local wayland_name=$1
  local out_file=$2
  for _ in $(seq 1 100); do
    if WAYLAND_DISPLAY="$wayland_name" wayland-info >"$out_file" 2>&1; then
      return 0
    fi
    sleep 0.05
  done
  WAYLAND_DISPLAY="$wayland_name" wayland-info >"$out_file" 2>&1
}

klocc_require_wayland_globals() {
  local info_file=$1
  awk '/wl_seat/ { seat=1 } /xdg_wm_base/ { xdg=1 } /wl_output/ { output=1 } END { exit !(seat && xdg && output) }' "$info_file"
}

klocc_find_weston_window() {
  local x_display=$1
  local width=$2
  local height=$3
  local out_file=$4
  DISPLAY="$x_display" xwininfo -root -tree >"$out_file"
  awk -v size="${width}x${height}" '$0 ~ size { print $1; exit }' "$out_file"
}

klocc_capture_window() {
  local x_display=$1
  local window=$2
  local out_dir=$3
  local name=$4
  DISPLAY="$x_display" xwd -id "$window" -silent -out "$out_dir/$name.xwd"
  magick "$out_dir/$name.xwd" "$out_dir/$name.png"
  magick identify -format '%[mean] %[standard-deviation] %wx%h' "$out_dir/$name.png" >"$out_dir/$name.stats"
}
