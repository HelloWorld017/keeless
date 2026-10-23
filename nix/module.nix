{ self }:
{ config, lib, pkgs, ... }:
let
  opts = config.programs.keeless;
  passkeyOpts = config.services.keeless-passkey;

  defaultUsers = builtins.filter
    (name: validUser name && config.users.users.${name}.isNormalUser)
    (builtins.attrNames config.users.users);

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
      ExecStart = "${passkeyOpts.package}/lib/keeless/resources/bin/keeless-passkey-linux run --desktop ${passkeyOpts.package}/bin/keeless";
      OpenFile = "/dev/uhid:uhid";
      Restart = "on-failure";
      RestartSec = 1;
    };
  };
in {
  options.programs.keeless = {
    enable = lib.mkEnableOption "keeless";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.keeless;
      description = "Desktop application package for keeless.";
    };
  };

  options.services.keeless-passkey = {
    enable = lib.mkEnableOption "the passkey provider for keeless";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.keeless;
      description = "Desktop application package for keeless.";
    };

    users = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = defaultUsers;
      example = [ "alice" ];
      description = "Users who are allowed to use the passkey provider.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf opts.enable {
      environment.systemPackages = [ opts.package ];
    })

    (lib.mkIf passkeyOpts.enable {
      assertions = map (name: {
        assertion = validUser name;
        message = "services.keeless-passkey: ${name} must exist and have an explicit non-root UID.";
      }) passkeyOpts.users;

      boot.kernelModules = [ "uhid" ];
      systemd.services = builtins.listToAttrs (
        map serviceFor (lib.unique (builtins.filter validUser passkeyOpts.users))
      );
    })
  ];
}
