{ config, lib, pkgs, ... }:

# Orbis Control — NixOS-модуль.
#
# Модуль запускает sessiond и узкий privileged Hardware1 helper. Hardware
# mutations остаются typed/capability-specific: никаких generic sysfs/path
# writers, а фактическая авторизация mutation выполняется внутри hardwared
# через отдельные polkit actions для исходного system-bus caller.

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
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "Orbis Control package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    security.polkit.enable = lib.mkDefault true;
    services.upower.enable = lib.mkDefault true;

    systemd.user.services.orbis-sessiond = {
      description = "Orbis Control session daemon";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "io.github.orbiscontrol.Session";
        ExecStart = "${cfg.package}/bin/orbis-sessiond";
        Restart = "on-failure";
        RestartSec = "2s";
      };
    };

    # orbis-hardwared: system (root) service with a closed typed Hardware1 API.
    # Only the currently enabled direct-sysfs production mutation is writable in
    # the sandbox. Other typed backends may exist in code but remain policy-
    # blocked until their release evidence is complete.
    systemd.services.orbis-hardwared = {
      description = "Orbis Control hardware helper";
      wantedBy = [ "multi-user.target" ];
      after = [ "dbus.service" ];
      requires = [ "dbus.service" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "io.github.orbiscontrol.Hardware";
        ExecStart = "${cfg.package}/bin/orbis-hardwared";
        Restart = "on-failure";
        RestartSec = "2s";
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        MemoryDenyWriteExecute = true;
        AmbientCapabilities = [ ];
        CapabilityBoundingSet = "";

        # `/sys` remains read-only. Performance platform_profile is the only
        # direct-sysfs mutation currently enabled by product policy. Keyboard
        # brightness remains read-only until its write path is release-validated
        # and intentionally re-enabled together with policy/capability evidence.
        ReadOnlyPaths = [ "/sys" ];
        ReadWritePaths = [
          "-/sys/firmware/acpi/platform_profile"
        ];
      };
    };

    environment.systemPackages = [ cfg.package ];

    # D-Bus system policy permits addressing Hardware1; each mutation is still
    # authorized inside hardwared via a per-capability polkit action.
    environment.etc."polkit-1/actions/io.github.orbiscontrol.hardware.policy".source =
      "${cfg.package}/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy";
  };
}
