{ self }:
{ config, lib, pkgs, ... }:
let
  opts = config.programs.keeless;
  passkeyOpts = config.services.keeless-passkey;

  userExists = name: builtins.hasAttr name config.users.users;
  isUserNonRoot = name:
    name != "root" && config.users.users.${name}.uid != 0;

  validUser = name:
    userExists name &&
    isUserNonRoot name &&
    config.users.users.${name}.uid != null;

  selectedUsers = if passkeyOpts.users == null then [] else passkeyOpts.users;

  serviceFor = name: let
    uid = toString config.users.users.${name}.uid;
  in lib.nameValuePair "keeless-passkey@${uid}" {
    overrideStrategy = "asDropin";
    wantedBy = [ "user@${uid}.service" ];
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
      type = lib.types.nullOr (lib.types.listOf lib.types.str);
      default = null;
      example = [ "alice" ];
      description = ''
        Users who are allowed to use the passkey provider. Each listed user must
        have an explicit non-root UID. If null, the provider starts for every
        non-root UID when its systemd user manager starts, including users with
        automatically assigned UIDs.
      '';
    };
  };

  config = lib.mkMerge [
    (lib.mkIf opts.enable {
      environment.systemPackages = [ opts.package ];
    })

    (lib.mkIf passkeyOpts.enable {
      assertions = [ {
        assertion = passkeyOpts.users != [];
        message = "services.keeless-passkey.users must be null or a non-empty list.";
      } ] ++ lib.concatMap (name: [
        {
          assertion = userExists name;
          message = "services.keeless-passkey: ${name} must exist.";
        }
        {
          assertion = !userExists name || isUserNonRoot name;
          message = "services.keeless-passkey: ${name} must not be root or have UID 0.";
        }
        {
          assertion = !userExists name || config.users.users.${name}.uid != null;
          message = "services.keeless-passkey: ${name} must have an explicit UID when listed in users.";
        }
      ]) selectedUsers;

      boot.kernelModules = [ "uhid" ];
      systemd.services = {
        "keeless-passkey@" = {
          description = "Keeless Passkey Provider for UID %i";
          wantedBy = lib.optional (passkeyOpts.users == null) "user@.service";
          after = [ "user@%i.service" ];
          bindsTo = [ "user@%i.service" ];
          environment.XDG_RUNTIME_DIR = "/run/user/%i";
          path = [ pkgs.systemd ];
          serviceConfig = {
            Type = "simple";
            User = "%i";
            ExecCondition = "${pkgs.coreutils}/bin/test %i -ne 0";
            ExecStart = "${passkeyOpts.package}/lib/keeless/resources/bin/keeless-passkey-linux run --desktop ${passkeyOpts.package}/bin/keeless";
            OpenFile = "/dev/uhid:uhid";
            Restart = "on-failure";
            RestartSec = 1;
          };
        };
      } // builtins.listToAttrs (
        map serviceFor (lib.unique (builtins.filter validUser selectedUsers))
      );
    })
  ];
}
