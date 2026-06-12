{
  pkgs,
  lib,
  kloccGui,
  guiArtifact,
}: let
  scenarioDir = ./scenarios;
  environments = import ./environments;
  scenarioFiles = lib.filterAttrs (
    name: type: type == "regular" && lib.hasSuffix ".nix" name
  ) (builtins.readDir scenarioDir);
  scenarios = map (
    name: import (scenarioDir + "/${name}")
  ) (builtins.attrNames scenarioFiles);
  scenarioChecks = lib.listToAttrs (lib.concatMap (
      scenario:
        map (environmentId: {
          name = "gui-snapshot-${scenario.id}-${environmentId}";
          value = environments.backends.${environmentId} {
            inherit pkgs kloccGui guiArtifact scenario;
          };
        })
        environments.order
    )
    scenarios);
  screenshotChecks =
    if pkgs.stdenv.isLinux
    then scenarioChecks
    else {};
in
  screenshotChecks
