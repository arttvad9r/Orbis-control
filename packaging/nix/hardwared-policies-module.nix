# orbis-hardwared-policies NixOS module — минимальная one-time интеграция.
#
# Регистрирует D-Bus system policy и polkit actions для orbis-hardwared.
# НЕ создаёт systemd service. НЕ включает services.orbis-control.
# Lifecycle демона управляется deploy-dev-hardwared.sh (standalone binary).
#
# Использование (одноразово в configuration.nix):
#
#   imports = [ inputs.orbis-control.nixosModules.orbis-hardwared-policies ];
#   services.orbis-hardwared-policies.enable = true;

{ config, lib, pkgs, ... }:

let
  cfg = config.services.orbis-hardwared-policies;
in
{
  options.services.orbis-hardwared-policies = {
    enable = lib.mkEnableOption "static D-Bus/polkit registration for orbis-hardwared";
  };

  config = lib.mkIf cfg.enable {
    # D-Bus system policy попадает в system-path/share/dbus-1/system.d/
    # через environment.systemPackages. dbus-daemon читает includedir
    # system-path (см. /etc/dbus-1/system.conf).
    environment.systemPackages = [
      pkgs.orbis-hardwared-policies
    ];

    # Polkit actions. /etc/polkit-1 — обычный каталог (не symlink),
    # environment.etc работает напрямую.
    environment.etc."polkit-1/actions/io.github.orbiscontrol.hardware.policy".source =
      "${pkgs.orbis-hardwared-policies}/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy";
  };
}
