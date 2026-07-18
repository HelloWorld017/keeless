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
        packages = [ pkgs.lld pkgs.wasm-pack ];
      };
    });
}
