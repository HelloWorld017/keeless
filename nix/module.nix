{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.keeless-passkey;
  validUser = name:
    builtins.hasAttr name config.users.users &&
    config.users.users.${name}.uid != null &&
    config.users.users.${name}.uid != 0;

  serviceFor = name: let
    uid = toString config.users.users.${name}.uid;

  in lib.nameValuePair "keeless-passkey@${uid}" {
    description = "Keeless Passkey Provider (${name})";
    wantedBy = [ "multi-user.target" ];
    after = [ "user@${uid}.service" ];
    requires = [ "user@${uid}.service" ];
    environment.XDG_RUNTIME_DIR = "/run/user/${uid}";
    path = [ pkgs.systemd ];
    serviceConfig = {
      Type = "simple";
      User = name;
      ExecStart = "${cfg.package}/lib/keeless/resources/bin/keeless-passkey-linux run --desktop ${cfg.package}/bin/keeless";
      OpenFile = "/dev/uhid:uhid";
      Restart = "on-failure";
      RestartSec = 1;
    };
  };
in {
  options.services.keeless-passkey = {
    enable = lib.mkEnableOption "the passkey provider for keeless";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.keeless;
      description = "Keeless desktop package containing the daemon and launch wrapper.";
    };

    users = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      example = [ "alice" ];
      description = "Desktop users with an explicit, non-zero users.users.<name>.uid.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = map (name: {
      assertion = validUser name;
      message = "services.keeless-passkey: ${name} must exist and have an explicit non-root UID.";
    }) cfg.users;

    boot.kernelModules = [ "uhid" ];
    environment.systemPackages = [ cfg.package ];
    systemd.services = builtins.listToAttrs (
      map serviceFor (lib.unique (builtins.filter validUser cfg.users))
    );
  };
}
