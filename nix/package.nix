{ pkgs, pkgsVersioned, craneLib, libBuildInputs }:
let
  inherit (pkgs) lib;
  inherit (pkgs) copyDesktopItems fetchPnpmDeps makeDesktopItem makeWrapper pnpmConfigHook;
  inherit (pkgsVersioned) pnpm nodejs electron;

  pname = "keeless";
  version = (lib.importJSON ../package.json).version;
  src = ../.;

in
  craneLib.buildPackage {
    inherit pname version src;
    strictDeps = true;
    cargoArtifacts = null;
    doCheck = false;
    doNotPostBuildInstallCargoBinaries = true;
    buildInputs = libBuildInputs;

    pnpmDeps = fetchPnpmDeps {
      inherit pname version src pnpm;
      fetcherVersion = 4;
      prePnpmInstall = ''
        pnpm config set network-concurrency 8
      '';
      CI = "true";
      hash = "sha256-KF8Z08n6XL1sb9ghOoEuT6dMebEdg5hu2C6nmYkJVrs=";
    };

    pnpmInstallFlags = [ "--prod=false" ];

    nativeBuildInputs = [
      nodejs
      pnpm
      pnpmConfigHook
      makeWrapper
      copyDesktopItems
      pkgs.wasm-pack
      pkgs.wasm-bindgen-cli_0_2_126
      pkgs.binaryen
      pkgs.lld
    ];

    ELECTRON_SKIP_BINARY_DOWNLOAD = "1";
    ELECTRON_OVERRIDE_DIST_PATH = electron.dist;
    NODE_ENV = "production";
    CI = "true";
    CARGO_NET_OFFLINE = "true";
    WASM_PACK_CACHE = ".wasm-pack";

    buildPhase = ''
      runHook preBuild

      pnpm --filter @keeless/desktop... run build
      pnpm --filter @keeless/desktop run dist:nix \
        --config.electronDist=${electron.dist} \
        --config.electronVersion=${electron.version}

      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall

      mkdir -p $out/lib/keeless $out/bin
      cp -r packages/desktop/release/linux*-unpacked/. $out/lib/keeless/
      makeWrapper $out/lib/keeless/keeless $out/bin/keeless \
        --prefix LD_LIBRARY_PATH : '${lib.makeLibraryPath libBuildInputs}' \
        --prefix PATH : '${lib.makeBinPath [ pkgs.systemd pkgs.polkit pkgs.kmod ]}' \
        --add-flags "\''${NIXOS_OZONE_WL:+--ozone-platform-hint=auto --enable-features=WaylandWindowDecorations --enable-wayland-ime}"

      install -Dm644 packages/desktop/src/assets/icons/icon.png \
        $out/share/icons/hicolor/512x512/apps/keeless.png

      runHook postInstall
    '';

    desktopItems = [
      (makeDesktopItem {
        name = "keeless";
        desktopName = "Keeless";
        comment = "keeless: modern password manager";
        exec = "keeless %U";
        icon = "keeless";
        categories = [ "Utility" "Security" ];
        startupWMClass = "keeless";
      })
    ];

    meta = {
      description = "keeless: modern password manager";
      homepage = "https://github.com/HelloWorld017/keeless";
      license = lib.licenses.agpl3Only;
      mainProgram = "keeless";
      platforms = lib.platforms.linux;
    };
  }
