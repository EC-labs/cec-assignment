{ 
    pkgs ? import <nixpkgs> {},
    crate2nixTools
}:
let 
    crateOverrides = {
        rdkafka-sys = attrs: {
            buildInputs = with pkgs; [
                cmake
                pkg-config
                openssl
                zlib
            ];
        };
        experiment-producer = attrs: {
            SQLX_OFFLINE="true";
        };
        notifications-service = attrs: {
            SQLX_OFFLINE="true";
        };
    };
    customBuildRustCrateForPkgs = pkgs: pkgs.buildRustCrate.override {
        defaultCrateOverrides = pkgs.defaultCrateOverrides // crateOverrides;
    };
    generatedCargoNix = import (
        (pkgs.callPackage crate2nixTools {}).generatedCargoNix {
          src = ./.;
          name = "cec-crate";
        }
    );
in pkgs.callPackage generatedCargoNix {
    buildRustCrateForPkgs = customBuildRustCrateForPkgs;
}
