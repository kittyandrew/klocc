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
        packageVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
        containerName = "klocc-server";
        imageTag = packageVersion;
        nano = seconds: seconds * 1000000000;

        craneLib =
          (inputs.crane.mkLib pkgs).overrideToolchain
          rustToolchain;

        klocc = craneLib.buildPackage {
          src = craneLib.cleanCargoSource ./.;
        };

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
              klocc
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
            Entrypoint = ["/bin/klocc"];
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
            inherit klocc kloccFrontend;
            klocc-frontend = kloccFrontend;
          }
          // lib.optionalAttrs pkgs.stdenv.isLinux {
            docker-image = dockerImage;
            klocc-server-image = dockerImage;
          };

        devShells.default = pkgs.mkShell {
          RUST_LOG = "info";
          packages = with pkgs; [
            actionlint
            alejandra
            deadnix
            docker
            git
            curl
            rustToolchain
            zizmor
          ];
        };
      };
    };
}
