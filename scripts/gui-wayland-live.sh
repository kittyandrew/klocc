#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ -f "$script_dir/gui-wayland-lib.sh" ]]; then
  # shellcheck disable=SC1091
  source "$script_dir/gui-wayland-lib.sh"
fi

state_dir=${KLOCC_GUI_LIVE_DIR:-/tmp/opencode/klocc-gui-live}
cmd=${1:-status}

mkdir -p "$state_dir"

load_env() {
  if [[ ! -f "$state_dir/env" ]]; then
    printf 'klocc-gui-wayland-live: no live session in %s\n' "$state_dir" >&2
    exit 2
  fi
  # shellcheck disable=SC1091
  source "$state_dir/env"
}

find_display() {
  for n in $(seq 90 140); do
    if [[ ! -S "/tmp/.X11-unix/X$n" ]]; then
      printf ':%s\n' "$n"
      return 0
    fi
  done
  printf 'klocc-gui-wayland-live: no free X display found\n' >&2
  exit 1
}

find_weston_window() {
  klocc_find_weston_window "$KLOCC_X_DISPLAY" "$KLOCC_WIDTH" "$KLOCC_HEIGHT" "$state_dir/xwininfo.txt"
}

capture() {
  load_env
  local name=${1:-capture}
  local window
  window=$(find_weston_window)
  if [[ -z "$window" ]]; then
    printf 'klocc-gui-wayland-live: Weston X11 output window not found\n' >&2
    exit 1
  fi
  klocc_capture_window "$KLOCC_X_DISPLAY" "$window" "$state_dir" "$name"
  printf '%s\n' "$state_dir/$name.png"
}

case "$cmd" in
  start)
    artifact=${2:-/tmp/klocc-self.sqlite}
    width=${KLOCC_GUI_LIVE_WIDTH:-1280}
    height=${KLOCC_GUI_LIVE_HEIGHT:-800}
    wayland_name=${KLOCC_GUI_LIVE_WAYLAND_DISPLAY:-wayland-klocc-live}

    if [[ ! -f "$artifact" ]]; then
      printf 'klocc-gui-wayland-live: artifact not found: %s\n' "$artifact" >&2
      exit 2
    fi

    if ! gui_bin=$(klocc_find_gui_bin); then
      printf 'klocc-gui-wayland-live: set KLOCC_GUI_BIN or build the GUI first\n' >&2
      exit 2
    fi

    if ! lavapipe_icd=$(klocc_find_lavapipe_icd); then
      printf 'klocc-gui-wayland-live: set VK_DRIVER_FILES or KLOCC_MESA_DIR for Lavapipe\n' >&2
      exit 2
    fi

    "$0" stop >/dev/null 2>&1 || true
    rm -rf "$state_dir"
    mkdir -p "$state_dir"
    runtime_dir=$(mktemp -d "$state_dir/runtime.XXXXXX")
    chmod 700 "$runtime_dir"
    x_display=$(find_display)

    Xvfb "$x_display" -screen 0 "${width}x${height}x24" >"$state_dir/xvfb.log" 2>&1 &
    xvfb_pid=$!
    sleep 0.5

    XDG_RUNTIME_DIR="$runtime_dir" DISPLAY="$x_display" weston \
      --backend=x11 \
      --renderer=pixman \
      --socket="$wayland_name" \
      --width="$width" \
      --height="$height" \
      >"$state_dir/weston.log" 2>&1 </dev/null &
    weston_pid=$!

    if ! klocc_wait_for_wayland_socket "$runtime_dir" "$wayland_name"; then
      printf 'klocc-gui-wayland-live: Weston did not create Wayland socket\n' >&2
      exit 1
    fi
    sleep 2
    if ! XDG_RUNTIME_DIR="$runtime_dir" klocc_wait_for_wayland_info "$wayland_name" "$state_dir/wayland-info.txt"; then
      printf 'klocc-gui-wayland-live: Weston did not become ready for Wayland clients\n' >&2
      exit 1
    fi
    if ! klocc_require_wayland_globals "$state_dir/wayland-info.txt"; then
      printf 'klocc-gui-wayland-live: Weston is missing required Wayland globals\n' >&2
      exit 1
    fi

    XDG_RUNTIME_DIR="$runtime_dir" VK_DRIVER_FILES="$lavapipe_icd" env -u DISPLAY \
      WAYLAND_DISPLAY="$wayland_name" "$gui_bin" "$artifact" >"$state_dir/gui.log" 2>&1 &
    gui_pid=$!
    sleep 3

    cat >"$state_dir/env" <<EOF
export KLOCC_ARTIFACT=$(printf '%q' "$artifact")
export KLOCC_GUI_BIN=$(printf '%q' "$gui_bin")
export KLOCC_X_DISPLAY=$(printf '%q' "$x_display")
export KLOCC_XVFB_PID=$xvfb_pid
export KLOCC_WESTON_PID=$weston_pid
export KLOCC_GUI_PID=$gui_pid
export KLOCC_RUNTIME_DIR=$(printf '%q' "$runtime_dir")
export KLOCC_WAYLAND_DISPLAY_NAME=$(printf '%q' "$wayland_name")
export KLOCC_WIDTH=$width
export KLOCC_HEIGHT=$height
export VK_DRIVER_FILES=$(printf '%q' "$lavapipe_icd")
EOF
    capture initial >/dev/null
    printf 'klocc-gui-wayland-live: started in %s\n' "$state_dir"
    ;;

  capture)
    capture "${2:-capture}"
    ;;

  click)
    load_env
    x=${2:?x coordinate required}
    y=${3:?y coordinate required}
    button=${4:-1}
    window=$(find_weston_window)
    DISPLAY="$KLOCC_X_DISPLAY" xdotool mousemove --window "$window" "$x" "$y" click "$button"
    ;;

  key)
    load_env
    key=${2:?key required}
    DISPLAY="$KLOCC_X_DISPLAY" xdotool key "$key"
    ;;

  status)
    load_env
    for pid_name in KLOCC_XVFB_PID KLOCC_WESTON_PID KLOCC_GUI_PID; do
      pid=${!pid_name}
      if kill -0 "$pid" 2>/dev/null; then
        printf '%s=%s running\n' "$pid_name" "$pid"
      else
        printf '%s=%s stopped\n' "$pid_name" "$pid"
      fi
    done
    ;;

  stop)
    if [[ -f "$state_dir/env" ]]; then
      load_env
      # shellcheck disable=SC2153
      kill "$KLOCC_GUI_PID" "$KLOCC_WESTON_PID" "$KLOCC_XVFB_PID" 2>/dev/null || true
    fi
    ;;

  *)
    printf 'usage: klocc-gui-wayland-live start [artifact] | capture [name] | click x y [button] | key KEY | status | stop\n' >&2
    exit 2
    ;;
esac
