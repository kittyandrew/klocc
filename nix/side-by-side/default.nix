{
  pkgs,
  lib,
  inputs,
  system,
  klocc,
  realProjectCheck,
}:
lib.optionalAttrs pkgs.stdenv.isLinux {
  real-self-scan = realProjectCheck {
    name = "self";
    target = "${klocc.drvPath}^out";
    forceRealized = klocc;
    minRuntimePaths = 1;
    maxRuntimePaths = 20;
    minSourceUnits = 100;
    minRuntimeLinkedSourceUnits = 1;
    minBuildTimeOnlySourceUnits = 50;
    minSourceUnitsWithLoc = 100;
    minSourceDependencyEdges = 50;
    minDerivations = 20;
    minDerivationSourceUnitLinks = 50;
    minTotalCodeLoc = 1000000;
    minDistinctSourceKinds = 3;
    minDistinctEcosystems = 2;
    expectSourceKinds = ["cargo-vendored-crate" "fixed-output-source" "source-like-derivation-output"];
    expectRealizationStatuses = ["available"];
  };
  real-waybap-scan = realProjectCheck {
    name = "waybap";
    target = "${inputs.waybap.packages.${system}.default.drvPath}^out";
    forceRealized = inputs.waybap.packages.${system}.default;
    minRuntimePaths = 1;
    maxRuntimePaths = 20;
    minSourceUnits = 1000;
    minRuntimeLinkedSourceUnits = 1;
    minBuildTimeOnlySourceUnits = 500;
    minSourceUnitsWithLoc = 500;
    minSourceDependencyEdges = 500;
    minDerivations = 100;
    minDerivationSourceUnitLinks = 500;
    minTotalCodeLoc = 1000000;
    minDistinctSourceKinds = 3;
    minDistinctEcosystems = 2;
    minBuildToRuntimeSourceRatio = 50;
    maxUnknownDerivationSources = 0;
    maxUnknownRuntimeDerivers = 0;
    expectSourceKinds = ["cargo-vendored-crate" "fixed-output-source" "source-like-derivation-output"];
    expectRealizationStatuses = ["available"];
    timeoutSeconds = 2400;
  };
  real-hyprland-scan = realProjectCheck {
    name = "hyprland";
    target = "${pkgs.hyprland.drvPath}^out";
    forceRealized = pkgs.hyprland;
    minRuntimePaths = 100;
    minSourceUnits = 500;
    minRuntimeLinkedSourceUnits = 1;
    minBuildTimeOnlySourceUnits = 250;
    minSourceUnitsWithLoc = 250;
    minSourceDependencyEdges = 100;
    minDerivations = 100;
    minDerivationSourceUnitLinks = 100;
    minTotalCodeLoc = 1000000;
    minDistinctSourceKinds = 3;
    minDistinctEcosystems = 2;
    minBuildToRuntimeSourceRatio = 3;
    minGeneratedOutputs = 1;
    maxUnknownRuntimeDerivers = 5;
    expectSourceKinds = ["generated-derivation-output" "source-like-derivation-output" "fixed-output-source"];
    expectRealizationStatuses = ["available"];
    timeoutSeconds = 2400;
  };
}
