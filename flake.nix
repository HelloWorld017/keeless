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
      desktopBuildInputs = with pkgs; [
        dbus
        glib
        gtk3
        libappindicator-gtk3
        libGL
        librsvg
        libsoup_3
        libxkbcommon
        openssl
        wayland
        webkitgtk_4_1
      ];
      desktopNativeBuildInputs = with pkgs; [ pkg-config wrapGAppsHook3 ];
      packageFor = cargoPackage:
        craneLib.buildPackage {
          pname = cargoPackage;
          version = "0.0.0";
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          cargoExtraArgs = "--package ${cargoPackage}";
          buildInputs = desktopBuildInputs;
          nativeBuildInputs = desktopNativeBuildInputs;
          meta.mainProgram = cargoPackage;
        };
    in {
      packages = {
        default = packageFor "keeless_desktop";
      };

      devShells.default = craneLib.devShell {
        packages = [ pkgs.lld pkgs.wasm-pack ] ++ desktopBuildInputs ++ desktopNativeBuildInputs;
      };
    });
}
