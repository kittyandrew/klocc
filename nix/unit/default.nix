{
  lib,
  craneLib,
  cargoSrc,
  kloccTestCargoArtifacts,
  sqliteNativeBuildInputs,
  sqliteBuildInputs,
}: {
  klocc-unit-property-tests = craneLib.cargoTest {
    pname = "klocc-unit-property-tests";
    src = cargoSrc;
    cargoArtifacts = kloccTestCargoArtifacts;
    cargoExtraArgs = "-p klocc";
    nativeBuildInputs = sqliteNativeBuildInputs;
    buildInputs = sqliteBuildInputs;
    LD_LIBRARY_PATH = lib.makeLibraryPath sqliteBuildInputs;
  };
}
