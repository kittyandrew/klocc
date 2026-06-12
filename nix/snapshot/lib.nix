{pkgs}: let
  inherit (pkgs) lib;
  pad2 = n:
    if n < 10
    then "0${toString n}"
    else toString n;
  stepStems = scenario: lib.imap0 (index: step: "${pad2 (index + 1)}-${step.id}") scenario.steps;
  stepArtifactFiles = scenario: lib.concatMap (stem: map (extension: "${stem}.${extension}") scenario.artifacts.perStepExtensions) (stepStems scenario);
  icdDir = "${pkgs.mesa}/share/vulkan/icd.d";
  icdFiles = builtins.attrNames (builtins.readDir icdDir);
  lavapipeMatches = builtins.filter (name: lib.hasPrefix "lvp_icd." name && lib.hasSuffix ".json" name) icdFiles;
in {
  inherit stepStems;
  firstStepStem = scenario: builtins.head (stepStems scenario);
  artifactFiles = scenario: (stepArtifactFiles scenario) ++ scenario.artifacts.common;
  lavapipeIcd = "${icdDir}/${builtins.head lavapipeMatches}";
}
