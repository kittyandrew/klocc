# Take from: https://github.com/sioodmy/barbie/blob/main/flake.nix
{
  description = "Custom data provider for Waybar/Hyprland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
    };
    waybap = {
      url = "github:kittyandrew/waybap";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin"];
      perSystem = {
        pkgs,
        system,
        ...
      }: let
        inherit (pkgs) lib;

        rustToolchain = inputs.fenix.packages.${system}.stable.withComponents [
          "cargo"
          "clippy"
          "rustc"
          "rustfmt"
        ];
        packageVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
        containerName = "kloccd-server";
        imageTag = packageVersion;
        nano = seconds: seconds * 1000000000;

        craneLib =
          (inputs.crane.mkLib pkgs).overrideToolchain
          rustToolchain;

        cargoSrc = craneLib.cleanCargoSource ./.;

        guiSystemDeps = lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          alsa-lib
          fontconfig
          freetype
          glib
          libdrm
          libgbm
          libglvnd
          libva
          libxcomposite
          libxcursor
          libxdamage
          libxext
          libxfixes
          libxi
          libxkbcommon
          libxrandr
          libx11
          libxcb
          vulkan-loader
          wayland
        ]);

        guiDevTools = lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          imagemagick
          mesa
          vulkan-tools
          wayland-utils
          weston
          xdotool
          xorg-server
          xvfb-run
          xwd
          xwininfo
        ]);

        guiPerfTools = lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          mesa
          wayland-utils
          weston
        ]);

        sqliteNativeBuildInputs = with pkgs; [pkg-config];
        sqliteBuildInputs = with pkgs; [sqlite];

        guiWaylandLive =
          if pkgs.stdenv.isLinux
          then
            pkgs.writeShellApplication {
              name = "klocc-gui-wayland-live";
              runtimeInputs = guiDevTools ++ (with pkgs; [bash coreutils gawk gnugrep]);
              text = ''
                export KLOCC_MESA_DIR=${pkgs.mesa}
                export KLOCC_GUI_BIN=''${KLOCC_GUI_BIN:-${kloccGui}/bin/klocc-gui}
                ${builtins.readFile ./scripts/gui-wayland-lib.sh}
                ${builtins.readFile ./scripts/gui-wayland-live.sh}
              '';
            }
          else null;

        guiWaylandPerf =
          if pkgs.stdenv.isLinux
          then
            pkgs.writeShellApplication {
              name = "klocc-gui-wayland-perf";
              runtimeInputs = guiPerfTools ++ (with pkgs; [bash coreutils gawk gnugrep]);
              text = ''
                export KLOCC_MESA_DIR=${pkgs.mesa}
                export KLOCC_GUI_BIN=''${KLOCC_GUI_BIN:-${kloccGui}/bin/klocc-gui}
                ${builtins.readFile ./scripts/gui-wayland-lib.sh}
                ${builtins.readFile ./nix/pre-observability/gui-wayland-perf-check.sh}
              '';
            }
          else null;

        recursiveNixConfig = ''
          experimental-features = nix-command flakes recursive-nix
        '';

        recursiveNixInputs = with pkgs; [bash coreutils gnugrep gnused nix sqlite];

        kloccTestCargoArtifacts = craneLib.buildDepsOnly {
          pname = "klocc-test-deps";
          src = cargoSrc;
          cargoExtraArgs = "-p klocc";
          nativeBuildInputs = sqliteNativeBuildInputs;
          buildInputs = sqliteBuildInputs;
        };

        realProjectCheck = {
          name,
          target,
          forceRealized ? null,
          minRuntimePaths ? 1,
          maxRuntimePaths ? null,
          minSourceUnits ? 1,
          minRuntimeLinkedSourceUnits ? 1,
          minBuildTimeOnlySourceUnits ? 0,
          minGeneratedOutputs ? 0,
          minSourceUnitsWithLoc ? 0,
          minSourceDependencyEdges ? 0,
          minDerivations ? 0,
          minDerivationSourceUnitLinks ? 0,
          minTotalCodeLoc ? 0,
          minDistinctSourceKinds ? 0,
          minDistinctEcosystems ? 0,
          minDistinctRealizationStatuses ? 0,
          minBuildToRuntimeSourceRatio ? null,
          maxUnknownDerivationSources ? null,
          maxUnknownRuntimeDerivers ? null,
          expectSourceKinds ? [],
          expectRealizationStatuses ? [],
          timeoutSeconds ? 1800,
        }:
          pkgs.runCommand "klocc-real-${name}-scan-check" {
            nativeBuildInputs = recursiveNixInputs ++ [klocc];
            requiredSystemFeatures = ["recursive-nix"];
            NIX_CONFIG = recursiveNixConfig;
            KLOCC_FORCE_REALIZED = lib.optionalString (forceRealized != null) (toString forceRealized);
            KLOCC_BIN = "${klocc}/bin/klocc";
            SQLITE_BIN = "${pkgs.sqlite}/bin/sqlite3";
            KLOCC_REAL_CHECK_TIMEOUT = toString timeoutSeconds;
            KLOCC_MIN_RUNTIME_PATHS = toString minRuntimePaths;
            KLOCC_MAX_RUNTIME_PATHS = lib.optionalString (maxRuntimePaths != null) (toString maxRuntimePaths);
            KLOCC_MIN_SOURCE_UNITS = toString minSourceUnits;
            KLOCC_MIN_RUNTIME_LINKED_SOURCE_UNITS = toString minRuntimeLinkedSourceUnits;
            KLOCC_MIN_BUILD_TIME_ONLY_SOURCE_UNITS = toString minBuildTimeOnlySourceUnits;
            KLOCC_MIN_GENERATED_OUTPUTS = toString minGeneratedOutputs;
            KLOCC_MIN_SOURCE_UNITS_WITH_LOC = toString minSourceUnitsWithLoc;
            KLOCC_MIN_SOURCE_DEPENDENCY_EDGES = toString minSourceDependencyEdges;
            KLOCC_MIN_DERIVATIONS = toString minDerivations;
            KLOCC_MIN_DERIVATION_SOURCE_UNIT_LINKS = toString minDerivationSourceUnitLinks;
            KLOCC_MIN_TOTAL_CODE_LOC = toString minTotalCodeLoc;
            KLOCC_MIN_DISTINCT_SOURCE_KINDS = toString minDistinctSourceKinds;
            KLOCC_MIN_DISTINCT_ECOSYSTEMS = toString minDistinctEcosystems;
            KLOCC_MIN_DISTINCT_REALIZATION_STATUSES = toString minDistinctRealizationStatuses;
            KLOCC_MIN_BUILD_TO_RUNTIME_SOURCE_RATIO = lib.optionalString (minBuildToRuntimeSourceRatio != null) (toString minBuildToRuntimeSourceRatio);
            KLOCC_MAX_UNKNOWN_DERIVATION_SOURCES = lib.optionalString (maxUnknownDerivationSources != null) (toString maxUnknownDerivationSources);
            KLOCC_MAX_UNKNOWN_RUNTIME_DERIVERS = lib.optionalString (maxUnknownRuntimeDerivers != null) (toString maxUnknownRuntimeDerivers);
            KLOCC_EXPECT_SOURCE_KINDS = lib.concatStringsSep " " expectSourceKinds;
            KLOCC_EXPECT_REALIZATION_STATUSES = lib.concatStringsSep " " expectRealizationStatuses;
          } ''
            bash ${./scripts/check-real-project.sh} ${lib.escapeShellArg name} ${lib.escapeShellArg (toString target)} "$out"
          '';

        guiScreenshotArtifact =
          if pkgs.stdenv.isLinux
          then
            pkgs.runCommand "klocc-gui-screenshot-artifact" {
              nativeBuildInputs = recursiveNixInputs ++ [klocc];
              requiredSystemFeatures = ["recursive-nix"];
              NIX_CONFIG = recursiveNixConfig;
            } ''
              mkdir -p "$out"
              artifact="$out/self.sqlite"
              if ! ${klocc}/bin/klocc scan ${lib.escapeShellArg "${klocc.drvPath}^out"} --out "$artifact" >"$out/scan.log" 2>&1; then
                cat "$out/scan.log" >&2
                exit 1
              fi
              if ! ${klocc}/bin/klocc check "$artifact" >"$out/check.log" 2>&1; then
                cat "$out/check.log" >&2
                exit 1
              fi
            ''
          else null;

        kloccdCargoArtifacts = craneLib.buildDepsOnly {
          pname = "kloccd-deps";
          src = cargoSrc;
          cargoExtraArgs = "-p kloccd";
        };

        kloccCargoArtifacts = craneLib.buildDepsOnly {
          pname = "klocc-deps";
          src = cargoSrc;
          cargoExtraArgs = "-p klocc";
          nativeBuildInputs = sqliteNativeBuildInputs;
          buildInputs = sqliteBuildInputs;
        };

        kloccGuiCargoArtifacts =
          if pkgs.stdenv.isLinux
          then
            craneLib.buildDepsOnly {
              pname = "klocc-gui-deps";
              src = cargoSrc;
              cargoExtraArgs = "-p klocc-gui";
              nativeBuildInputs = sqliteNativeBuildInputs;
              buildInputs = sqliteBuildInputs ++ guiSystemDeps;
            }
          else null;

        kloccd = craneLib.buildPackage {
          pname = "kloccd";
          src = cargoSrc;
          cargoArtifacts = kloccdCargoArtifacts;
          cargoExtraArgs = "-p kloccd";
        };

        klocc = craneLib.buildPackage {
          pname = "klocc";
          src = cargoSrc;
          cargoArtifacts = kloccCargoArtifacts;
          cargoExtraArgs = "-p klocc";
          nativeBuildInputs = sqliteNativeBuildInputs ++ (with pkgs; [makeWrapper]);
          buildInputs = sqliteBuildInputs;
          postInstall = ''
            wrapProgram $out/bin/klocc \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath sqliteBuildInputs}
          '';
        };

        kloccGui =
          if pkgs.stdenv.isLinux
          then
            craneLib.buildPackage {
              pname = "klocc-gui";
              src = cargoSrc;
              cargoArtifacts = kloccGuiCargoArtifacts;
              cargoExtraArgs = "-p klocc-gui";
              nativeBuildInputs = sqliteNativeBuildInputs ++ (with pkgs; [makeWrapper]);
              buildInputs = sqliteBuildInputs ++ guiSystemDeps;
              postInstall = ''
                wrapProgram $out/bin/klocc-gui \
                  --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (sqliteBuildInputs ++ guiSystemDeps)}
              '';
            }
          else null;

        kloccFrontend = pkgs.buildNpmPackage {
          pname = "klocc-frontend";
          version = "0096318";
          src = pkgs.fetchFromGitHub {
            owner = "Katerynaru4";
            repo = "klocc-frontend";
            rev = "0096318db1a3c5a75d5b8257163a742179a71b0e";
            hash = "sha256-IB7H4z5Vip29+eu0ZjyZvy6E+6uQ2yTQ3zpJ8T8mnP8=";
          };
          nodejs = pkgs.nodejs;
          npmDepsHash = "sha256-HU1wX+Lgl097b3YVfdKN5g3dACraow97fu+mrLdYlkg=";
          NODE_OPTIONS = "--openssl-legacy-provider";

          installPhase = ''
            runHook preInstall
            mkdir -p $out
            cp -r dist/. $out/
            runHook postInstall
          '';
        };

        dockerImage = pkgs.dockerTools.buildImage {
          name = containerName;
          tag = imageTag;

          copyToRoot = pkgs.buildEnv {
            name = "image-root";
            paths = [
              kloccd
              pkgs.cacert
              pkgs.curl
              pkgs.git
            ];
            pathsToLink = ["/bin" "/etc"];
          };

          extraCommands = ''
            mkdir -m 1777 tmp
            cp ${./Rocket.toml} Rocket.toml
          '';

          config = {
            WorkingDir = "/";
            Entrypoint = ["/bin/kloccd"];
            Env = ["SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"];
            ExposedPorts = {"8080/tcp" = {};};
            Healthcheck = {
              Test = ["CMD" "curl" "-sf" "0.0.0.0:8080/api/health"];
              Interval = nano 60;
              Timeout = nano 3;
            };
          };
        };
      in {
        formatter = pkgs.alejandra;

        packages =
          {
            default = klocc;
            inherit klocc kloccd kloccFrontend;
            klocc-frontend = kloccFrontend;
          }
          // lib.optionalAttrs pkgs.stdenv.isLinux {
            docker-image = dockerImage;
            gui-wayland-live = guiWaylandLive;
            gui-wayland-perf = guiWaylandPerf;
            inherit kloccGui;
            klocc-gui = kloccGui;
            kloccd-server-image = dockerImage;
          };

        checks = import ./nix/default.nix {
          inherit
            pkgs
            lib
            inputs
            system
            craneLib
            cargoSrc
            klocc
            kloccGui
            kloccTestCargoArtifacts
            sqliteNativeBuildInputs
            sqliteBuildInputs
            realProjectCheck
            guiWaylandPerf
            ;
          guiArtifact = guiScreenshotArtifact;
        };

        devShells.default = pkgs.mkShell {
          RUST_LOG = "info";
          LD_LIBRARY_PATH = lib.optionalString pkgs.stdenv.isLinux (lib.makeLibraryPath guiSystemDeps);
          packages =
            (with pkgs; [
              actionlint
              alejandra
              deadnix
              docker
              git
              curl
              pkg-config
              rustToolchain
              sqlite
              xorg-server
              zizmor
            ])
            ++ lib.optionals pkgs.stdenv.isLinux guiSystemDeps
            ++ guiDevTools
            ++ lib.optionals pkgs.stdenv.isLinux [guiWaylandLive guiWaylandPerf];
        };
      };
    };
}
