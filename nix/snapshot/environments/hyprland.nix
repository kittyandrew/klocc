{
  pkgs,
  kloccGui,
  guiArtifact,
  scenario,
}: let
  snapshot = import ../lib.nix {inherit pkgs;};
  user = "alice";
  uid = 1000;
  environmentId = "hyprland";
in
  pkgs.testers.runNixOSTest {
    name = "klocc-gui-snapshot-${scenario.id}-${environmentId}";
    skipTypeCheck = true;

    nodes.machine = {...}: {
      imports = [(pkgs.path + "/nixos/tests/common/user-account.nix")];

      services.getty.autologinUser = user;
      programs.hyprland.enable = true;

      environment = {
        systemPackages = with pkgs; [
          coreutils
          gawk
          gnugrep
          grim
          hyprland
          imagemagick
          kloccGui
          mesa
          procps
          # @NOTE: May 25, 2026 - Hyprland's test VM has no built-in Hyprcursor theme, so without one it falls back to a compiled-in 32x32 cursor. Add a real Hyprcursor theme so HYPRCURSOR_SIZE controls cursor size while preserving Hyprland's cursor path.
          rose-pine-hyprcursor
          sqlite
          vulkan-loader
          wayland-utils
          wlrctl
        ];
        variables = {
          AQ_TRACE = "1";
          HYPRLAND_TRACE = "1";
          XDG_CACHE_HOME = "/tmp";
        };
      };

      environment.etc."hypr/hyprland.conf".text = ''
        monitor = , ${toString scenario.screen.width}x${toString scenario.screen.height}@60, 0x0, 1
        exec-once = touch /tmp/hyprland-config-loaded

        general {
          border_size = 0
          gaps_in = 0
          gaps_out = 0
        }

        decoration {
          rounding = 0
          shadow {
            enabled = false
          }
        }

        animations {
          enabled = false
        }

        misc {
          disable_hyprland_logo = false
          disable_splash_rendering = false
        }

        cursor {
          enable_hyprcursor = true
        }

        debug {
          disable_logs = false
        }

        windowrule = float true, size ${toString scenario.app.width} ${toString scenario.app.height}, center true, match:class .*
      '';

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
        qemu.options = ["-vga none -device virtio-gpu-pci"];
      };
    };

    testScript = ''
      import json
      import shlex

      q = shlex.quote
      app_left = ${(toString ((scenario.screen.width - scenario.app.width) / 2))}
      app_top = ${(toString ((scenario.screen.height - scenario.app.height) / 2))}
      hyprland_signature = ""
      scenario_id = ${builtins.toJSON scenario.id}
      artifact_files = ${builtins.toJSON (snapshot.artifactFiles scenario)}
      scenario_steps = ${builtins.toJSON scenario.steps}
      scenario_assertions = json.loads(${builtins.toJSON (builtins.toJSON (scenario.assertions or {}))})
      sqlite3_bin = "${pkgs.sqlite}/bin/sqlite3"
      gui_artifact_path = "${guiArtifact}/self.sqlite"
      screen_size = "${toString scenario.screen.width}x${toString scenario.screen.height}"
      screen_width = ${toString scenario.screen.width}
      screen_height = ${toString scenario.screen.height}


      def step_stem(index, step):
          return str(index).zfill(2) + "-" + step["id"]


      def as_user(command):
          env = (
              "XDG_RUNTIME_DIR=/run/user/${toString uid} "
              + "WAYLAND_DISPLAY=" + q(wayland) + " "
              + ("HYPRLAND_INSTANCE_SIGNATURE=" + q(hyprland_signature) + " " if hyprland_signature else "")
          )
          return machine.succeed("su - ${user} -c " + q(env + command))


      def copy_artifacts():
          for file in artifact_files:
              path = "/tmp/" + file
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
              + "printf 'backend.compositor=Hyprland\\n'; "
              + "printf 'backend.capture=grim -c\\n'; "
              + "printf 'backend.input=hyprctl movecursor + wlrctl pointer click\\n'; "
              + "printf 'backend.renderer=kms_swrast\\n'; "
              + "} > /tmp/environment.log"
          )


      ${builtins.readFile ../assertions.py}


      def pointer_move(x, y):
          as_user("hyprctl dispatch movecursor " + str(x) + " " + str(y))
          cursorpos = json.loads(as_user("hyprctl cursorpos -j"))
          if cursorpos.get("x") != x or cursorpos.get("y") != y:
              raise Exception("cursor did not move to " + str((x, y)) + ": " + str(cursorpos))


      def pointer_click(button):
          as_user("wlrctl pointer click " + button)


      def key_press(key):
          as_user("wlrctl keyboard type " + q(key))


      def pointer_move_app(point):
          pointer_move(app_left + point[0], app_top + point[1])


      def capture(stem):
          png = "/tmp/" + stem + ".png"
          stats = "/tmp/" + stem + ".stats"
          as_user("grim -c " + png)
          machine.succeed("test -s " + q(png))
          machine.succeed("magick identify -format '%[mean] %[standard-deviation] %wx%h' " + q(png) + " > " + q(stats))
          assert_image_stats(stats)


      def build_walkthrough():
          images = ["/tmp/" + step_stem(index, step) + ".png" for index, step in enumerate(scenario_steps, start=1)]
          build_walkthrough_montage(images, "/tmp/walkthrough.png", "/tmp/walkthrough.stats")


      def run_scenario():
          pointer_move_app([0, 0])
          previous_rects = initialize_rect_manifest_cursor("/tmp/klocc-gui.log", "/tmp")
          for index, step in enumerate(scenario_steps, start=1):
              before_line = log_line_count("/tmp/klocc-gui.log")
              for action in step["actions"]:
                  op = action["op"]
                  if op == "move":
                      pointer_move_app(action["point"])
                      machine.sleep(action.get("sleep", 0))
                  elif op == "click":
                      pointer_move_app(action["point"])
                      pointer_click(action.get("button", "left"))
                      machine.sleep(action.get("sleep", 0))
                  elif op == "key":
                      key_press(action["key"])
                      machine.sleep(action.get("sleep", 0))
                  elif op == "sleep":
                      machine.sleep(action.get("seconds", 0))
                  else:
                      raise Exception("unsupported scenario action: " + op)
              capture(step_stem(index, step))
              previous_rects = extract_step_rects(
                  "/tmp/klocc-gui.log",
                  "/tmp",
                  step_stem(index, step),
                  step,
                  before_line,
                  previous_rects,
              )


      start_all()
      try:
          machine.wait_for_unit("multi-user.target", timeout=120)
          machine.wait_for_file("/run/user/${toString uid}", timeout=60)

          machine.succeed("test -e /dev/dri/card0")
          machine.succeed("test -e /dev/dri/renderD128")
          environment_log_header()
          machine.succeed("chmod 666 /tmp/environment.log")
          assert_artifact_contract()
          machine.succeed("printf '\\n[drm]\\ncommand=ls -la /dev/dri\\n' >> /tmp/environment.log")
          machine.succeed("ls -la /dev/dri >> /tmp/environment.log 2>&1")

          machine.succeed("printf '\\n[compositor]\\ncommand=Hyprland -c /etc/hypr/hyprland.conf\\n' >> /tmp/environment.log")
          machine.succeed(
              "su - ${user} -c "
              + q(
                  "XDG_RUNTIME_DIR=/run/user/${toString uid} "
                  + "AQ_TRACE=1 HYPRLAND_TRACE=1 XDG_CACHE_HOME=/tmp "
                  + "HYPRCURSOR_THEME=rose-pine-hyprcursor HYPRCURSOR_SIZE=16 "
                  + "nohup Hyprland -c /etc/hypr/hyprland.conf "
                  + ">>/tmp/environment.log 2>&1 </dev/null &"
              )
          )
          machine.sleep(5)
          machine.wait_until_succeeds("pgrep -u ${user} -af 'Hyprland -c /etc/hypr/hyprland.conf'", timeout=60)
          machine.wait_until_succeeds("test -f /tmp/hyprland-config-loaded", timeout=60)
          machine.wait_until_succeeds("test -S /run/user/${toString uid}/wayland-0 || test -S /run/user/${toString uid}/wayland-1", timeout=60)
          machine.wait_until_succeeds("grep -q 'kms_swrast' /tmp/environment.log", timeout=60)

          hyprland_signature = machine.succeed(
              "for dir in /run/user/${toString uid}/hypr/*; do "
              + "[ -d \"$dir\" ] && basename \"$dir\" && exit 0; "
              + "done; exit 1"
          ).strip()

          wayland = machine.succeed(
              "for socket in /run/user/${toString uid}/wayland-[0-9]; do "
              + "[ -S \"$socket\" ] && basename \"$socket\" && exit 0; "
              + "done; exit 1"
          ).strip()

          machine.succeed("printf '\\n[wayland-info]\\ncommand=wayland-info\\n' >> /tmp/environment.log")
          as_user("wayland-info >> /tmp/environment.log 2>&1")
          machine.succeed("grep -q zwlr_screencopy_manager_v1 /tmp/environment.log")
          machine.succeed("grep -q zwlr_virtual_pointer_manager_v1 /tmp/environment.log")
          machine.succeed("grep -q zwp_linux_dmabuf_v1 /tmp/environment.log")

          as_user(
              "env "
              + "VK_DRIVER_FILES=${snapshot.lavapipeIcd} "
              + "KLOCC_GUI_WINDOW_WIDTH=${toString scenario.app.width} "
              + "KLOCC_GUI_WINDOW_HEIGHT=${toString scenario.app.height} "
              + "${kloccGui}/bin/klocc-gui ${guiArtifact}/self.sqlite "
              + ">/tmp/klocc-gui.log 2>&1 &"
          )

          machine.sleep(3)
          machine.wait_until_succeeds("pgrep -u ${user} -f klocc-gui", timeout=60)
          machine.wait_until_succeeds("grep -q 'klocc-gui: loaded' /tmp/klocc-gui.log", timeout=90)
          machine.wait_until_succeeds("grep -q 'klocc-gui: paint quality' /tmp/klocc-gui.log", timeout=90)
          machine.wait_until_succeeds("grep -q '^rect manifest end' /tmp/klocc-gui.log", timeout=90)

          run_scenario()
          assert_gui_telemetry("/tmp/klocc-gui.log")
          assert_layout_artifacts("/tmp")

          build_walkthrough()
          machine.succeed("printf '\\n[walkthrough.stats]\\n' >> /tmp/environment.log")
          machine.succeed("cat /tmp/walkthrough.stats >> /tmp/environment.log")

          machine.succeed("grep -q '${toString scenario.screen.width}x${toString scenario.screen.height}' /tmp/${snapshot.firstStepStem scenario}.stats")
          machine.log(machine.succeed("cat /tmp/walkthrough.stats"))
      finally:
          copy_artifacts()
          machine.execute("pkill -u ${user} -f klocc-gui || true")
          machine.execute("pkill -u ${user} -f 'Hyprland -c /etc/hypr/hyprland.conf' || true")
    '';
  }
