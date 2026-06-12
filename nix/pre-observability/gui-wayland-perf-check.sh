#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ -f "$script_dir/gui-wayland-lib.sh" ]]; then
  # shellcheck disable=SC1091
  source "$script_dir/gui-wayland-lib.sh"
elif [[ -f "$script_dir/../../scripts/gui-wayland-lib.sh" ]]; then
  # shellcheck disable=SC1091
  source "$script_dir/../../scripts/gui-wayland-lib.sh"
fi

artifact=${1:-/tmp/klocc-self.sqlite}
out_dir=${2:-/tmp/opencode/klocc-gui-wayland-perf-$(date +%Y%m%d-%H%M%S)}
width=${KLOCC_GUI_PERF_WIDTH:-1920}
height=${KLOCC_GUI_PERF_HEIGHT:-1080}
app_width=${KLOCC_GUI_PERF_APP_WIDTH:-$((width - 160))}
app_height=${KLOCC_GUI_PERF_APP_HEIGHT:-$((height - 140))}
socket_name=${KLOCC_GUI_PERF_WAYLAND_DISPLAY:-wayland-klocc-perf}
backend=${KLOCC_GUI_PERF_BACKEND:-headless}
refresh_rate=${KLOCC_GUI_PERF_REFRESH_RATE:-120000}
max_layout_ms=${KLOCC_GUI_PERF_MAX_LAYOUT_MS:-50}
max_paint_ms=${KLOCC_GUI_PERF_MAX_PAINT_MS:-5}
max_response_ms=${KLOCC_GUI_PERF_MAX_RESPONSE_MS:-20}
max_response_layout_ms=${KLOCC_GUI_PERF_MAX_RESPONSE_LAYOUT_MS:-5}
max_response_paint_ms=${KLOCC_GUI_PERF_MAX_RESPONSE_PAINT_MS:-5}
max_response_app_ms=${KLOCC_GUI_PERF_MAX_RESPONSE_APP_MS:-8}
min_states=${KLOCC_GUI_PERF_MIN_STATES:-4}

if [[ ! -f "$artifact" ]]; then
  printf 'klocc-gui-wayland-perf: artifact not found: %s\n' "$artifact" >&2
  exit 2
fi

if ! gui_bin=$(klocc_find_gui_bin); then
  printf 'klocc-gui-wayland-perf: set KLOCC_GUI_BIN or build .#klocc-gui first\n' >&2
  exit 2
fi

if [[ ! -x "$gui_bin" ]]; then
  printf 'klocc-gui-wayland-perf: GUI binary is not executable: %s\n' "$gui_bin" >&2
  exit 2
fi

if ! lavapipe_icd=$(klocc_find_lavapipe_icd); then
  printf 'klocc-gui-wayland-perf: could not find Mesa Lavapipe Vulkan ICD; set VK_DRIVER_FILES or KLOCC_MESA_DIR\n' >&2
  exit 2
fi

if [[ "$backend" != headless ]]; then
  printf 'klocc-gui-wayland-perf: unsupported backend %s; perf uses Weston headless with the app-internal driver\n' "$backend" >&2
  exit 2
fi

mkdir -p "$out_dir"
runtime_parent=${TMPDIR:-/tmp}
runtime_dir=$(mktemp -d "$runtime_parent/klocc-gui-perf-runtime.XXXXXX")
chmod 700 "$runtime_dir"
home_dir=$(mktemp -d "$runtime_parent/klocc-gui-perf-home.XXXXXX")
cache_dir=$(mktemp -d "$runtime_parent/klocc-gui-perf-cache.XXXXXX")

export ARTIFACT="$artifact"
export GUI_BIN="$gui_bin"
export OUT_DIR="$out_dir"
export WIDTH="$width"
export HEIGHT="$height"
export APP_WIDTH="$app_width"
export APP_HEIGHT="$app_height"
export KLOCC_GUI_WINDOW_WIDTH="$app_width"
export KLOCC_GUI_WINDOW_HEIGHT="$app_height"
export KLOCC_WAYLAND_DISPLAY_NAME="$socket_name"
export KLOCC_GUI_PERF_BACKEND="$backend"
export KLOCC_GUI_PERF_REFRESH_RATE="$refresh_rate"
export KLOCC_GUI_PERF_MAX_LAYOUT_MS="$max_layout_ms"
export KLOCC_GUI_PERF_MAX_PAINT_MS="$max_paint_ms"
export KLOCC_GUI_PERF_MAX_RESPONSE_MS="$max_response_ms"
export KLOCC_GUI_PERF_MAX_RESPONSE_LAYOUT_MS="$max_response_layout_ms"
export KLOCC_GUI_PERF_MAX_RESPONSE_PAINT_MS="$max_response_paint_ms"
export KLOCC_GUI_PERF_MAX_RESPONSE_APP_MS="$max_response_app_ms"
export KLOCC_GUI_PERF_MIN_STATES="$min_states"
export KLOCC_GUI_PROFILE=1
export XDG_RUNTIME_DIR="$runtime_dir"
export HOME="$home_dir"
export XDG_CACHE_HOME="$cache_dir"
export VK_DRIVER_FILES="$lavapipe_icd"

weston_pid=

cleanup() {
  if [[ -n "$weston_pid" ]] && kill -0 "$weston_pid" 2>/dev/null; then
    kill "$weston_pid" 2>/dev/null || true
    wait "$weston_pid" 2>/dev/null || true
  fi
  rm -rf "$runtime_dir"
  rm -rf "$home_dir"
  rm -rf "$cache_dir"
}
trap cleanup EXIT

weston \
  --backend=headless \
  --renderer=pixman \
  --socket="$KLOCC_WAYLAND_DISPLAY_NAME" \
  --width="$WIDTH" \
  --height="$HEIGHT" \
  --refresh-rate="$KLOCC_GUI_PERF_REFRESH_RATE" \
  --fake-seat \
  --no-config \
  >"$OUT_DIR/weston.log" 2>&1 </dev/null &
weston_pid=$!

if ! klocc_wait_for_wayland_socket "$XDG_RUNTIME_DIR" "$KLOCC_WAYLAND_DISPLAY_NAME"; then
  printf 'klocc-gui-wayland-perf: Weston did not create Wayland socket\n' >&2
  exit 1
fi

sleep 2

if ! klocc_wait_for_wayland_info "$KLOCC_WAYLAND_DISPLAY_NAME" "$OUT_DIR/wayland-info.txt"; then
  printf 'klocc-gui-wayland-perf: Weston did not become ready for Wayland clients\n' >&2
  exit 1
fi

if ! klocc_require_wayland_globals "$OUT_DIR/wayland-info.txt"; then
  printf 'klocc-gui-wayland-perf: Weston is missing required Wayland globals\n' >&2
  exit 1
fi

: >"$OUT_DIR/gui.log"
env -u DISPLAY \
  WAYLAND_DISPLAY="$KLOCC_WAYLAND_DISPLAY_NAME" \
  KLOCC_GUI_INTERNAL_PERF=1 \
  KLOCC_GUI_INTERNAL_PERF_QUIT=1 \
  "$GUI_BIN" "$ARTIFACT" >>"$OUT_DIR/gui.log" 2>&1

gawk -v max_layout="$KLOCC_GUI_PERF_MAX_LAYOUT_MS" \
  -v max_paint="$KLOCC_GUI_PERF_MAX_PAINT_MS" \
  -v max_response="$KLOCC_GUI_PERF_MAX_RESPONSE_MS" \
  -v max_response_layout="$KLOCC_GUI_PERF_MAX_RESPONSE_LAYOUT_MS" \
  -v max_response_paint="$KLOCC_GUI_PERF_MAX_RESPONSE_PAINT_MS" \
  -v max_response_app="$KLOCC_GUI_PERF_MAX_RESPONSE_APP_MS" \
  -v min_states="$KLOCC_GUI_PERF_MIN_STATES" '
  /^klocc-gui-perf: state / {
    state = $3;
    phase = $4;
    if (phase == "begin") {
      current = state;
      seen[state] = 1;
      order[++order_count] = state;
    } else if (phase == "end" && current == state) {
      current = "";
    }
    next;
  }
  current != "" && /klocc-gui: layout/ {
    layout_count[current]++;
    rects = 0;
    ms = 0;
    for (i = 1; i <= NF; i++) {
      if ($i == "rects") rects = $(i - 1) + 0;
      if ($i ~ /^[0-9.]+ms$/) {
        value = $i;
        sub(/ms$/, "", value);
        ms = value + 0;
      }
    }
    if (rects > max_rects[current]) max_rects[current] = rects;
    if (ms > max_layout_seen[current]) max_layout_seen[current] = ms;
    next;
  }
  current != "" && /klocc-gui: paint quality/ {
    paint_count[current]++;
    rects = $5 + 0;
    labels = $7 + 0;
    ms = 0;
    for (i = 1; i <= NF; i++) {
      if ($i == "in" && $(i + 1) ~ /^[0-9.]+ms$/) {
        value = $(i + 1);
        sub(/ms$/, "", value);
        ms = value + 0;
      }
    }
    if (rects > max_rects[current]) max_rects[current] = rects;
    if (labels > max_labels[current]) max_labels[current] = labels;
    if (ms > max_paint_seen[current]) max_paint_seen[current] = ms;
    next;
  }
  current != "" && /klocc-gui: paint profile/ {
    profile_count[current]++;
    for (i = 1; i <= NF; i++) {
      split($i, parts, "=");
      key = parts[1];
      value = parts[2];
      sub(/ms$/, "", value);
      if (key == "fill" && value + 0 > max_fill_seen[current]) max_fill_seen[current] = value + 0;
      if (key == "labels" && value + 0 > max_label_seen[current]) max_label_seen[current] = value + 0;
      if (key == "fit" && value + 0 > max_fit_seen[current]) max_fit_seen[current] = value + 0;
      if (key == "value" && value + 0 > max_value_seen[current]) max_value_seen[current] = value + 0;
      if (key == "text" && value + 0 > max_text_seen[current]) max_text_seen[current] = value + 0;
      if (key == "shape" && value + 0 > max_shape_seen[current]) max_shape_seen[current] = value + 0;
      if (key == "shapes" && value + 0 > max_shapes_seen[current]) max_shapes_seen[current] = value + 0;
    }
    next;
  }
  current != "" && /klocc-gui: response / {
    response_count[current]++;
    before_layout = 0;
    between_layout_paint = 0;
    response_layout = 0;
    response_paint = 0;
    response_total = 0;
    for (i = 1; i <= NF; i++) {
      split($i, parts, "=");
      key = parts[1];
      value = parts[2];
      sub(/ms$/, "", value);
      if (key == "before_layout") before_layout = value + 0;
      if (key == "between_layout_paint") between_layout_paint = value + 0;
      if (key == "layout") response_layout = value + 0;
      if (key == "paint") response_paint = value + 0;
      if (key == "total") response_total = value + 0;
      if (key == "before_paint" && value + 0 > max_before_paint_seen[current]) max_before_paint_seen[current] = value + 0;
    }
    response_app = response_layout + between_layout_paint + response_paint;
    if (before_layout > max_before_layout_seen[current]) max_before_layout_seen[current] = before_layout;
    if (between_layout_paint > max_between_layout_paint_seen[current]) max_between_layout_paint_seen[current] = between_layout_paint;
    if (response_layout > max_response_layout_seen[current]) max_response_layout_seen[current] = response_layout;
    if (response_paint > max_response_paint_seen[current]) max_response_paint_seen[current] = response_paint;
    if (response_total > max_response_seen[current]) max_response_seen[current] = response_total;
    if (response_app > max_response_app_seen[current]) max_response_app_seen[current] = response_app;
    next;
  }
  END {
    printf "state layout_count paint_count response_count max_rects max_labels max_layout_ms max_paint_ms max_response_ms max_response_app_ms max_before_layout_ms max_between_layout_paint_ms max_before_paint_ms response_layout_ms response_paint_ms max_fill_ms max_label_ms max_fit_ms max_value_ms max_text_ms max_shape_ms max_shapes\n";
    failures = 0;
    unique_states = 0;
    for (i = 1; i <= order_count; i++) {
      state = order[i];
      if (reported[state]++) continue;
      unique_states++;
      printf "%s %d %d %d %d %d %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %.3f %d\n", state, layout_count[state] + 0, paint_count[state] + 0, response_count[state] + 0, max_rects[state] + 0, max_labels[state] + 0, max_layout_seen[state] + 0, max_paint_seen[state] + 0, max_response_seen[state] + 0, max_response_app_seen[state] + 0, max_before_layout_seen[state] + 0, max_between_layout_paint_seen[state] + 0, max_before_paint_seen[state] + 0, max_response_layout_seen[state] + 0, max_response_paint_seen[state] + 0, max_fill_seen[state] + 0, max_label_seen[state] + 0, max_fit_seen[state] + 0, max_value_seen[state] + 0, max_text_seen[state] + 0, max_shape_seen[state] + 0, max_shapes_seen[state] + 0;
      if (paint_count[state] == 0) {
        printf "FAIL %s had no paint telemetry\n", state > "/dev/stderr";
        failures++;
      }
      if (profile_count[state] == 0) {
        printf "FAIL %s had no paint profile telemetry\n", state > "/dev/stderr";
        failures++;
      }
      if (max_layout_seen[state] > max_layout) {
        printf "FAIL %s layout %.3fms exceeded %.3fms\n", state, max_layout_seen[state], max_layout > "/dev/stderr";
        failures++;
      }
      if (max_paint_seen[state] > max_paint) {
        printf "FAIL %s paint %.3fms exceeded %.3fms\n", state, max_paint_seen[state], max_paint > "/dev/stderr";
        failures++;
      }
      if (max_response_seen[state] > max_response) {
        printf "FAIL %s response %.3fms exceeded %.3fms\n", state, max_response_seen[state], max_response > "/dev/stderr";
        failures++;
      }
      if (max_response_layout_seen[state] > max_response_layout) {
        printf "FAIL %s response layout %.3fms exceeded %.3fms\n", state, max_response_layout_seen[state], max_response_layout > "/dev/stderr";
        failures++;
      }
      if (max_response_paint_seen[state] > max_response_paint) {
        printf "FAIL %s response paint %.3fms exceeded %.3fms\n", state, max_response_paint_seen[state], max_response_paint > "/dev/stderr";
        failures++;
      }
      if (max_response_app_seen[state] > max_response_app) {
        printf "FAIL %s response app %.3fms exceeded %.3fms\n", state, max_response_app_seen[state], max_response_app > "/dev/stderr";
        failures++;
      }
    }
    if (unique_states < min_states) {
      printf "FAIL expected at least %d states, got %d\n", min_states, unique_states > "/dev/stderr";
      failures++;
    }
    exit failures ? 1 : 0;
  }
' "$OUT_DIR/gui.log" >"$OUT_DIR/perf-summary.txt"

printf 'klocc-gui-wayland-perf: ok\n'
printf '  output: %s\n' "$OUT_DIR"
printf '  backend: %s\n' "$KLOCC_GUI_PERF_BACKEND"
if [[ "$KLOCC_GUI_PERF_BACKEND" == headless ]]; then
  printf '  refresh rate: %smHz\n' "$KLOCC_GUI_PERF_REFRESH_RATE"
fi
printf '  max layout threshold: %sms\n' "$KLOCC_GUI_PERF_MAX_LAYOUT_MS"
printf '  max paint threshold: %sms\n' "$KLOCC_GUI_PERF_MAX_PAINT_MS"
printf '  max response threshold: %sms\n' "$KLOCC_GUI_PERF_MAX_RESPONSE_MS"
printf '  max response layout threshold: %sms\n' "$KLOCC_GUI_PERF_MAX_RESPONSE_LAYOUT_MS"
printf '  max response paint threshold: %sms\n' "$KLOCC_GUI_PERF_MAX_RESPONSE_PAINT_MS"
printf '  max response app threshold: %sms\n' "$KLOCC_GUI_PERF_MAX_RESPONSE_APP_MS"
cat "$OUT_DIR/perf-summary.txt"
