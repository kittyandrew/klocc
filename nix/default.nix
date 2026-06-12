{
  pkgs,
  lib,
  inputs,
  system,
  craneLib,
  cargoSrc,
  klocc,
  kloccGui,
  kloccTestCargoArtifacts,
  sqliteNativeBuildInputs,
  sqliteBuildInputs,
  guiArtifact,
  realProjectCheck,
  guiWaylandPerf,
}: let
  unit = import ./unit/default.nix {
    inherit lib craneLib cargoSrc kloccTestCargoArtifacts sqliteNativeBuildInputs sqliteBuildInputs;
  };
  sideBySide = import ./side-by-side/default.nix {
    inherit pkgs lib inputs system klocc realProjectCheck;
  };
  snapshot = import ./snapshot/default.nix {
    inherit pkgs lib kloccGui guiArtifact;
  };
  preObservability = import ./pre-observability/default.nix {
    inherit pkgs guiArtifact guiWaylandPerf;
  };
in
  unit // sideBySide // snapshot // preObservability
