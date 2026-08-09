{ config, lib, pkgs, ... }:

# Orbis Control — NixOS-модуль.
#
# ВНИМАНИЕ (Этап 2): модуль НЕ включает реальных аппаратных операций.
# Он только регистрирует опции, которые будут использованы на поздних этапах,
# и (по желанию) позволяет запускать демон в mock-режиме.
# Никаких записей в sysfs, никаких манипуляций с asusd/ppd из этого модуля.

let
  cfg = config.services.orbis-control;
in
{
  options.services.orbis-control = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable the Orbis Control user session service (orbis-sessiond).";
    };

    package = lib.mkOption {
      type = lib.types.package;
      # Самодостаточный default: собираем пакет через consumer nixpkgs,
      # без требования overlay/pkgs.orbis-control.
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "Orbis Control package to use.";
    };

    mockDevice = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        If set, run the session daemon in MOCK mode with the given device profile
        (e.g. "zephyrus-full"). This is for development only and performs NO
        hardware operations.
      '';
    };

    readOnlyEmpty = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Run session daemon in --read-only-empty mode (no hardware access at all).";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.orbis-sessiond = {
      description = "Orbis Control session daemon";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        # Daemon сам захватывает имя: Type=dbus + BusName — корректный
        # readiness condition для systemd.
        Type = "dbus";
        BusName = "io.github.orbiscontrol.Session";
        ExecStart = lib.escapeShellArgs (
          [ "${cfg.package}/bin/orbis-sessiond" ]
          ++ lib.optional (cfg.mockDevice != null) [ "--mock-device" cfg.mockDevice ]
          ++ lib.optional cfg.readOnlyEmpty [ "--read-only-empty" ]
        );
        # Не агрессивный restart loop; systemd шлёт SIGTERM, runtime его обрабатывает.
        Restart = "on-failure";
        RestartSec = "2s";
      };
    };
  };
}
