{
  pkgs,
  kloccGui,
  guiArtifact,
  scenario,
}: let
  snapshot = import ../lib.nix {inherit pkgs;};
  environmentId = "weston";
  user = "alice";
  uid = 1000;
in
  pkgs.testers.runNixOSTest {
    name = "klocc-gui-snapshot-${scenario.id}-${environmentId}";
    skipTypeCheck = true;

    nodes.machine = {...}: {
      imports = [(pkgs.path + "/nixos/tests/common/user-account.nix")];

      services.getty.autologinUser = user;

      environment.systemPackages = with pkgs; [
        coreutils
        gawk
        gnugrep
        imagemagick
        kloccGui
        mesa
        sqlite
        vulkan-loader
        wayland-utils
        weston
        xdotool
        xorg-server
        xwininfo
      ];

      fonts.packages = with pkgs; [
        dejavu_fonts
        font-awesome
        noto-fonts
      ];

      virtualisation = {
        cores = 4;
        memorySize = 8192;
        resolution = {
          x = scenario.screen.width;
          y = scenario.screen.height;
        };
      };
    };

    testScript = ''
      import json
      import re
      import shlex

      q = shlex.quote
      x_display = ":99"
      wayland = "wayland-klocc-e2e"
      out_dir = "/tmp/klocc-${environmentId}-${scenario.id}"
      app_origin = {"x": 0, "y": 0}
      scenario_id = ${builtins.toJSON scenario.id}
      artifact_files = ${builtins.toJSON (snapshot.artifactFiles scenario)}
      scenario_steps = ${builtins.toJSON scenario.steps}
      scenario_assertions = json.loads(${builtins.toJSON (builtins.toJSON (scenario.assertions or {}))})
      sqlite3_bin = "${pkgs.sqlite}/bin/sqlite3"
      gui_artifact_path = "${guiArtifact}/self.sqlite"
      screen_size = "${toString scenario.screen.width}x${toString scenario.screen.height}"
      screen_width = ${toString scenario.screen.width}
      screen_height = ${toString scenario.screen.height}
      weston_window = ""


      def step_stem(index, step):
          return str(index).zfill(2) + "-" + step["id"]


      def as_user(command):
          return machine.succeed("su - ${user} -c " + q(command))


      def as_user_x(command):
          return as_user("DISPLAY=" + q(x_display) + " " + command)


      def as_user_wayland(command):
          env = "XDG_RUNTIME_DIR=/run/user/${toString uid} WAYLAND_DISPLAY=" + q(wayland) + " "
          return as_user(env + command)


      def append_environment(line):
          machine.succeed("printf '%s\n' " + q(line) + " >> " + q(out_dir + "/environment.log"))


      def copy_artifacts():
          machine.execute("mkdir -p " + q(out_dir))
          for file in artifact_files:
              path = out_dir + "/" + file
              machine.execute("test -e " + q(path) + " || touch " + q(path))
              machine.copy_from_machine(path)


      def environment_log_header():
          machine.succeed(
              "{ "
              + "printf '[scenario]\\n'; "
              + "printf 'scenario.id=${scenario.id}\\n'; "
              + "printf 'environment.id=${environmentId}\\n'; "
              + "printf 'screen.size=${toString scenario.screen.width}x${toString scenario.screen.height}\\n'; "
              + "printf 'app.size=${toString scenario.app.width}x${toString scenario.app.height}\\n'; "
              + "printf '\\n[backend]\\n'; "
              + "printf 'backend.compositor=Weston X11 backend\\n'; "
              + "printf 'backend.capture=weston-screenshooter + ImageMagick\\n'; "
              + "printf 'backend.input=xdotool against calibrated Weston X11 output coordinates\\n'; "
              + "printf 'backend.renderer=pixman\\n'; "
              + "printf 'backend.prime=pointer-warmup\\n'; "
              + "} > " + q(out_dir + "/environment.log")
          )


      ${builtins.readFile ../assertions.py}


      def find_weston_window():
          machine.succeed("printf '\\n[xwininfo]\\ncommand=xwininfo -root -tree\\n' >> " + q(out_dir + "/environment.log"))
          machine.succeed("DISPLAY=" + q(x_display) + " xwininfo -root -tree >> " + q(out_dir + "/environment.log") + " 2>&1")
          tree = machine.succeed("DISPLAY=" + q(x_display) + " xwininfo -root -tree")
          size = "${toString scenario.screen.width}x${toString scenario.screen.height}"
          for line in tree.splitlines():
              if size in line:
                  return line.split()[0]
          raise Exception("could not find Weston X11 output window in xwininfo")


      def pointer_move_app(point):
          as_user_x(
              "xdotool mousemove --window "
              + q(weston_window)
              + " "
              + str(app_origin["x"] + point[0])
              + " "
              + str(app_origin["y"] + point[1])
          )


      def key_press(key):
          as_user_x(
              "xdotool windowfocus --sync "
              + q(weston_window)
              + " keyup --clearmodifiers "
              + q(key)
              + " key --clearmodifiers --delay 100 "
              + q(key)
          )


      def pointer_click(button):
          number = {"left": "1", "right": "3"}.get(button)
          if number is None:
              raise Exception("unsupported button: " + button)
          as_user_x("xdotool click " + number)


      def click_app(point, button):
          pointer_move_app(point)
          pointer_click(button)


      def pointer_event_count():
          pattern = "wl_pointer#[0-9]+\\.(enter|motion)\\("
          return int(machine.succeed("grep -Ec " + q(pattern) + " " + q(out_dir + "/klocc-gui.log") + " || true"))


      def latest_pointer_position():
          pattern = "wl_pointer#[0-9]+\\.(enter|motion)\\("
          line = machine.succeed("grep -E " + q(pattern) + " " + q(out_dir + "/klocc-gui.log") + " | tail -n 1").strip()
          match = re.search(r", (-?\d+(?:\.\d+)?), (-?\d+(?:\.\d+)?)\)$", line)
          if match is None:
              raise Exception("could not parse Weston pointer position from: " + line)
          return (float(match.group(1)), float(match.group(2)))


      def calibrate_app_origin():
          probe_x = screen_width // 2
          probe_y = screen_height // 2
          before = pointer_event_count()
          as_user_x("xdotool mousemove --window " + q(weston_window) + " " + str(probe_x) + " " + str(probe_y))
          pattern = "wl_pointer#[0-9]+\\.(enter|motion)\\("
          machine.wait_until_succeeds(
              "test $(grep -Ec "
              + q(pattern)
              + " "
              + q(out_dir + "/klocc-gui.log")
              + " || true) -gt "
              + str(before),
              timeout=10,
          )
          pointer_x, pointer_y = latest_pointer_position()
          app_origin["x"] = int(round(probe_x - pointer_x))
          app_origin["y"] = int(round(probe_y - pointer_y))
          append_environment(
              "[calibration] probe="
              + str((probe_x, probe_y))
              + " pointer="
              + str((pointer_x, pointer_y))
              + " app_origin="
              + str((app_origin["x"], app_origin["y"]))
          )


      def capture(stem):
          png = out_dir + "/" + stem + ".png"
          stats = out_dir + "/" + stem + ".stats"
          sync_presented_frame()
          before_count = int(machine.succeed("find " + q(out_dir) + " -maxdepth 1 -name 'wayland-screenshot-*.png' -print | wc -l"))
          as_user_x("xdotool key --clearmodifiers Super+s")
          machine.wait_until_succeeds(
              "test $(find "
              + q(out_dir)
              + " -maxdepth 1 -name 'wayland-screenshot-*.png' -print | wc -l) -gt "
              + str(before_count),
              timeout=30,
          )
          screenshots = sorted(machine.succeed("find " + q(out_dir) + " -maxdepth 1 -name 'wayland-screenshot-*.png' -print").splitlines())
          machine.succeed("mv " + q(screenshots[-1]) + " " + q(png))
          machine.succeed("magick identify -format '%[mean] %[standard-deviation] %wx%h' " + q(png) + " > " + q(stats))
          assert_image_stats(stats)


      def paint_count():
          return int(machine.succeed("grep -c '^klocc-gui: paint quality rects' " + q(out_dir + "/klocc-gui.log")))


      def surface_commit_count():
          pattern = "mesa vk display queue.*wl_surface#[0-9]+\\.commit\\(\\)"
          return int(machine.succeed("grep -Ec " + q(pattern) + " " + q(out_dir + "/klocc-gui.log") + " || true"))


      def wait_for_next_paint(previous_count):
          machine.execute(
              "for attempt in $(seq 1 50); do "
              + "test $(grep -c '^klocc-gui: paint quality rects' "
              + q(out_dir + "/klocc-gui.log")
              + ") -gt "
              + str(previous_count)
              + " && exit 0; "
              + "sleep 0.1; "
              + "done; exit 0"
          )


      def wait_for_next_surface_commit(previous_count):
          pattern = "mesa vk display queue.*wl_surface#[0-9]+\\.commit\\(\\)"
          machine.execute(
              "for attempt in $(seq 1 50); do "
              + "test $(grep -Ec "
              + q(pattern)
              + " "
              + q(out_dir + "/klocc-gui.log")
              + " || true) -gt "
              + str(previous_count)
              + " && exit 0; "
              + "sleep 0.1; "
              + "done; exit 0"
          )


      def sync_presented_frame():
          before_commit = surface_commit_count()
          as_user_x("xdotool mousemove_relative --sync -- 1 0 mousemove_relative --sync -- -1 0")
          wait_for_next_surface_commit(before_commit)


      def build_walkthrough():
          images = [out_dir + "/" + step_stem(index, step) + ".png" for index, step in enumerate(scenario_steps, start=1)]
          build_walkthrough_montage(images, out_dir + "/walkthrough.png", out_dir + "/walkthrough.stats")


      def prime_canvas_pointer():
          pointer_move_app([0, 0])
          machine.sleep(0.2)


      def run_scenario():
          prime_canvas_pointer()
          previous_rects = initialize_rect_manifest_cursor(out_dir + "/klocc-gui.log", out_dir)
          for index, step in enumerate(scenario_steps, start=1):
              append_environment("[step " + str(index) + " " + step["id"] + " begin]")
              before_paint = paint_count()
              before_line = log_line_count(out_dir + "/klocc-gui.log")
              for action in step["actions"]:
                  op = action["op"]
                  if op == "move":
                      pointer_move_app(action["point"])
                      machine.sleep(action.get("sleep", 0))
                  elif op == "click":
                      click_app(action["point"], action.get("button", "left"))
                      machine.sleep(action.get("sleep", 0))
                  elif op == "key":
                      key_press(action["key"])
                      machine.sleep(action.get("sleep", 0))
                  elif op == "sleep":
                      machine.sleep(action.get("seconds", 0))
                  else:
                      raise Exception("unsupported scenario action: " + op)
              wait_for_next_paint(before_paint)
              append_environment("[step " + str(index) + " " + step["id"] + " after-paint]")
              capture(step_stem(index, step))
              previous_rects = extract_step_rects(
                  out_dir + "/klocc-gui.log",
                  out_dir,
                  step_stem(index, step),
                  step,
                  before_line,
                  previous_rects,
              )


      start_all()
      try:
          machine.wait_for_unit("multi-user.target", timeout=120)
          machine.wait_for_file("/run/user/${toString uid}", timeout=60)
          machine.succeed("rm -rf " + q(out_dir))
          machine.succeed("mkdir -p " + q(out_dir))
          machine.succeed("chown ${user} " + q(out_dir))
          environment_log_header()
          machine.succeed("chown ${user} " + q(out_dir + "/environment.log"))
          assert_artifact_contract()

          machine.succeed("printf '\\n[xvfb]\\ncommand=Xvfb " + x_display + " -screen 0 ${toString scenario.screen.width}x${toString scenario.screen.height}x24\\n' >> " + q(out_dir + "/environment.log"))
          as_user("Xvfb " + q(x_display) + " -screen 0 ${toString scenario.screen.width}x${toString scenario.screen.height}x24 >>" + q(out_dir + "/environment.log") + " 2>&1 &")
          machine.wait_until_succeeds("DISPLAY=" + q(x_display) + " xwininfo -root >/dev/null", timeout=30)

          machine.succeed("printf '\\n[compositor]\\ncommand=weston --backend=x11 --renderer=pixman\\n' >> " + q(out_dir + "/environment.log"))
          as_user(
              "XDG_RUNTIME_DIR=/run/user/${toString uid} DISPLAY="
              + q(x_display)
              + " XDG_PICTURES_DIR="
              + q(out_dir)
              + " weston --backend=x11 --renderer=pixman --socket="
              + q(wayland)
              + " --width=${toString scenario.screen.width} --height=${toString scenario.screen.height} >>"
              + q(out_dir + "/environment.log")
              + " 2>&1 </dev/null &"
          )
          machine.wait_until_succeeds("test -S /run/user/${toString uid}/" + q(wayland), timeout=60)
          machine.sleep(2)

          machine.succeed("printf '\\n[wayland-info]\\ncommand=wayland-info\\n' >> " + q(out_dir + "/environment.log"))
          as_user_wayland("wayland-info >> " + q(out_dir + "/environment.log") + " 2>&1")
          machine.succeed("awk '/wl_seat/ { seat=1 } /xdg_wm_base/ { xdg=1 } /wl_output/ { output=1 } END { exit !(seat && xdg && output) }' " + q(out_dir + "/environment.log"))

          as_user_wayland(
              "env -u DISPLAY "
              + "WAYLAND_DEBUG=1 "
              + "VK_DRIVER_FILES=${snapshot.lavapipeIcd} "
              + "KLOCC_GUI_WINDOW_WIDTH=${toString scenario.app.width} "
              + "KLOCC_GUI_WINDOW_HEIGHT=${toString scenario.app.height} "
              + "${kloccGui}/bin/klocc-gui ${guiArtifact}/self.sqlite >"
              + q(out_dir + "/klocc-gui.log")
              + " 2>&1 &"
          )

          machine.wait_until_succeeds("pgrep -u ${user} -f klocc-gui", timeout=60)
          machine.wait_until_succeeds("grep -q '^rect manifest end' " + q(out_dir + "/klocc-gui.log"), timeout=90)
          machine.wait_until_succeeds("grep -q '^klocc-gui: paint quality rects' " + q(out_dir + "/klocc-gui.log"), timeout=90)

          weston_window = find_weston_window()
          calibrate_app_origin()
          run_scenario()
          assert_gui_telemetry(out_dir + "/klocc-gui.log")
          assert_layout_artifacts(out_dir)
          build_walkthrough()

          machine.succeed("printf '\\n[walkthrough.stats]\\n' >> " + q(out_dir + "/environment.log"))
          machine.succeed("cat " + q(out_dir + "/walkthrough.stats") + " >> " + q(out_dir + "/environment.log"))
          machine.succeed("grep -q '${toString scenario.screen.width}x${toString scenario.screen.height}' " + q(out_dir + "/${snapshot.firstStepStem scenario}.stats"))
          machine.log(machine.succeed("cat " + q(out_dir + "/walkthrough.stats")))
      finally:
          copy_artifacts()
          machine.execute("pkill -u ${user} -f klocc-gui || true")
          machine.execute("pkill -u ${user} -f 'weston --backend=x11' || true")
          machine.execute("pkill -u ${user} -f 'Xvfb :99' || true")
    '';
  }
