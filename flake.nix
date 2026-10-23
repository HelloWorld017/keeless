{
  description = "workspace for keeless";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.systems.follows = "systems";
    };

    systems.url = "github:nix-systems/default-linux";
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, ... }: let
    default = flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs { inherit system; };
      pkgsVersioned = {
        nodejs = pkgs.nodejs_24;
        electron = pkgs.electron_42;
        pnpm = pkgs.pnpm_11;
      };

      rustToolchain = let
        inherit (fenix.packages.${system}) combine stable targets;
      in combine [
        stable.toolchain
        stable.rust-src
        targets.wasm32-unknown-unknown.stable.rust-std
      ];

      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
      libBuildInputs = with pkgs; [
        libayatana-appindicator
        libGL
        libxkbcommon
        libX11
        libXcursor
        libXi
        libXrandr
        libxcb
        wayland
      ];
    in {
      packages = rec {
        keeless = import ./nix/package.nix {
          inherit craneLib libBuildInputs pkgs pkgsVersioned;
        };
        default = keeless;
      };

      devShells.default = craneLib.devShell {
        packages = []
          ++ [
            pkgs.libarchive
            pkgs.lld
            pkgs.wasm-pack
            pkgs.wasm-bindgen-cli_0_2_126
          ]
          ++ libBuildInputs
          ++ (builtins.attrValues pkgsVersioned);

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libBuildInputs;
        ELECTRON_OVERRIDE_DIST_PATH = "${pkgs.electron_42}/bin";
        ELECTRON_SKIP_BINARY_DOWNLOAD = "1";
      };
    });
  in {
    inherit (default) packages devShells;
    nixosModules.default = import ./nix/module.nix { inherit self; };
  };
}
