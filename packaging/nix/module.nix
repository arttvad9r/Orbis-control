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
      # Самодостаточный default: собираем пакет через consumer nixpkgs,
      # без требования overlay/pkgs.orbis-control.
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "Orbis Control package to use.";
    };

    # Compatibility options retained so existing configurations fail with an
    # explicit message instead of silently changing meaning. Current sessiond
    # does not parse the historical argv flags; see issue #122.
    mockDevice = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        Legacy development option. Temporarily unsupported: current
        orbis-sessiond does not implement --mock-device. Setting this option
        causes NixOS evaluation to fail instead of silently running production
        discovery.
      '';
    };

    readOnlyEmpty = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Legacy development option. Temporarily unsupported: current
        orbis-sessiond does not implement --read-only-empty. Setting this option
        causes NixOS evaluation to fail instead of silently performing production
        reads.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.mockDevice == null;
        message = "services.orbis-control.mockDevice is currently unsupported: orbis-sessiond does not parse --mock-device (see Orbis issue #122)";
      }
      {
        assertion = !cfg.readOnlyEmpty;
        message = "services.orbis-control.readOnlyEmpty is currently unsupported: orbis-sessiond does not parse --read-only-empty (see Orbis issue #122)";
      }
    ];

    security.polkit.enable = lib.mkDefault true;
    services.upower.enable = lib.mkDefault true;

    systemd.user.services.orbis-sessiond = {
      description = "Orbis Control session daemon";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        # Daemon сам захватывает имя: Type=dbus + BusName — корректный
        # readiness condition для systemd.
        Type = "dbus";
        BusName = "io.github.orbiscontrol.Session";
        ExecStart = "${cfg.package}/bin/orbis-sessiond";
        # Не агрессивный restart loop; systemd шлёт SIGTERM, runtime его обрабатывает.
        Restart = "on-failure";
        RestartSec = "2s";
      };
    };

    # orbis-hardwared: system (root) service with a closed typed Hardware1 API.
    # Каждая mutation capability имеет отдельный backend/polkit action; helper
    # не принимает произвольные пути, методы или generic filesystem writes.
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
        # Sandbox (threat-model §3.3). В этой nixpkgs нет structured
        # sandboxing options — задаём raw systemd settings.
        # ProtectKernelTunables НЕ используется: /sys открывается на write
        # точечно через ReadWritePaths внутри ReadOnlyPaths (man systemd.exec).
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        MemoryDenyWriteExecute = true;
        AmbientCapabilities = [ ];
        # Пустая строка (НЕ пустой список): NixOS отбрасывает пустые списки,
        # а systemd интерпретирует `CapabilityBoundingSet=` (без значения) как
        # сброс bounding set в пустое множество.
        CapabilityBoundingSet = "";
        # /sys остаётся read-only; writable только два точных атрибута.
        # platform_profile_choices и keyboard max_brightness остаются read-only.
        # Префикс "-": путь игнорируется, если файл отсутствует, но НЕ расширяет
        # writable surface при его наличии.
        ReadOnlyPaths = [ "/sys" ];
        ReadWritePaths = [
          "-/sys/firmware/acpi/platform_profile"
          "-/sys/class/leds/asus::kbd_backlight/brightness"
        ];
      };
    };

    # Пакет попадает в system.path (environment.systemPackages): dbus-daemon
    # читает includedir system-path/share/dbus-1/system.d (см. system.conf).
    # systemd.packages НЕ подходит: он добавляет пакеты только в
    # /etc/systemd hooks, не в system.path.
    environment.systemPackages = [ cfg.package ];

    # D-Bus system policy только разрешает владение destination/calls;
    # mutation authorization выполняется внутри hardwared через polkit.

    # Per-capability polkit actions for Hardware1 mutations.
    # Individual defaults can be fail-closed while a write contract is blocked.
    # /etc/polkit-1 — обычный каталог (не symlink), environment.etc работает.
    environment.etc."polkit-1/actions/io.github.orbiscontrol.hardware.policy".source =
      "${cfg.package}/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy";
  };
}