{
  description = "workspace for keeless";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.systems.follows = "systems";
    };
    systems.url = "github:nix-systems/default-linux";
  };

  outputs = { nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs { inherit system; };
      craneLib = crane.mkLib pkgs;
      libBuildInputs = with pkgs; [
        libayatana-appindicator
        libGL
        libxkbcommon
        wayland
      ];
      desktopBuildInputs = with pkgs; (libBuildInputs ++ [
        electron
      ]);
      packageFor = cargoPackage:
        craneLib.buildPackage {
          pname = cargoPackage;
          version = "0.0.0";
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          cargoExtraArgs = "--package ${cargoPackage}";
          meta.mainProgram = cargoPackage;
        };
    in {
      packages = {
        default = packageFor "keeless_desktop";
      };

      devShells.default = craneLib.devShell {
        packages = [ pkgs.lld pkgs.wasm-pack ] ++ desktopBuildInputs;

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libBuildInputs;
        ELECTRON_OVERRIDE_DIST_PATH = "${pkgs.electron_42}/bin";
        ELECTRON_SKIP_BINARY_DOWNLOAD = "1";
      };
    });
}
