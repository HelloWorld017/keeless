{ pkgs, craneLib, libBuildInputs }:
let
  inherit (pkgs) lib stdenv;
  inherit (pkgs) copyDesktopItems fetchPnpmDeps makeDesktopItem makeWrapper pnpmConfigHook;

  pname = "keeless";
  version = (lib.importJSON ../package.json).version;
  src = ../.;

  nodejs = pkgs.nodejs_24;
  electron = pkgs.electron_42;
  pnpm = pkgs.pnpm_11;

  cargoArgs = {
    pname = "${pname}-native";
    inherit version;
    src = craneLib.cleanCargoSource src;
    strictDeps = true;
    cargoExtraArgs = "-p keeless_host_desktop -p keeless_native_ui -p keeless_passkey_linux";
    buildInputs = libBuildInputs;
    doCheck = false;
  };

  native = craneLib.buildPackage (cargoArgs // {
    cargoArtifacts = craneLib.buildDepsOnly cargoArgs;
    installPhaseCommand = ''
      install -Dm755 target/release/libkeeless_host_desktop.so $out/lib/keeless_host_desktop.node
      install -Dm755 target/release/keeless-native-ui $out/bin/keeless-native-ui
      install -Dm755 target/release/keeless-passkey-linux $out/bin/keeless-passkey-linux
    '';
  });
in
  stdenv.mkDerivation (finalAttrs: {
    inherit pname version src;
    strictDeps = true;

  pnpmDeps = fetchPnpmDeps {
    inherit pname version src pnpm;
    fetcherVersion = 4;
    hash = "sha256-SckX/b8BTPOJRyK60ocUG58+nQsyXX7EgPKY9L96T9A=";
  };

  nativeBuildInputs = [
    nodejs
    pnpm
    pnpmConfigHook
    makeWrapper
    copyDesktopItems
  ];

  ELECTRON_SKIP_BINARY_DOWNLOAD = "1";
  ELECTRON_OVERRIDE_DIST_PATH = electron.dist;
  NODE_ENV = "production";

  buildPhase = ''
    runHook preBuild

    mkdir -p packages/host-desktop/dist target/release
    cp ${native}/lib/keeless_host_desktop.node packages/host-desktop/dist/
    cp ${native}/bin/* target/release/

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

  disallowedReferences = [ native ];

  meta = {
    description = "keeless: modern password manager";
    homepage = "https://github.com/HelloWorld017/keeless";
    license = lib.licenses.agpl3Only;
    mainProgram = "keeless";
    platforms = lib.platforms.linux;
  };
})
