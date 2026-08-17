# orbis-hardwared-policies — статические D-Bus и polkit файлы для
# orbis-hardwared. НЕ содержит бинарник и НЕ определяет systemd service.
#
# Использование (одноразовая NixOS-интеграция):
#
#   environment.systemPackages = [ inputs.orbis-control.packages.${system}.orbis-hardwared-policies ];
#
# Это делает:
#   - D-Bus system policy видимой для dbus-daemon
#     (system-path/share/dbus-1/system.d/)
#   - Polkit actions видимыми для polkitd
#     (через environment.etc; см. snippet ниже)
#
# Lifecycle демона (service, restart, upgrade) НЕ управляется этим пакетом
# и НЕ управляется NixOS-модулем services.orbis-control.
# Демон деплоится через deploy-dev-hardwared.sh (standalone binary).

{ lib
, runCommand
}:

runCommand "orbis-hardwared-policies" {
  meta.description = "D-Bus policy + polkit actions for orbis-hardwared (no binary)";
} ''
  # D-Bus system policy
  install -Dm644 ${./dbus/io.github.orbiscontrol.Hardware.conf} \
    $out/share/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf

  # Polkit actions
  install -Dm644 ${./polkit/io.github.orbiscontrol.hardware.policy} \
    $out/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy
''
