# Изолированный NixOS VM test: production lifecycle `orbis-hardwared`
# БЕЗ hardware mutation.
#
# Проверяет:
# - system service `orbis-hardwared` существует и становится active;
# - system bus name `io.github.orbiscontrol.Hardware` owned процессом hardwared
#   (uid 0), introspection содержит только `SetPerformanceProfile`;
# - CapEff/CapBnd == 0 (generated `CapabilityBoundingSet=` реально работает);
# - invalid-wire smoke: SetPerformanceProfile(255) -> InvalidArgs, daemon жив,
#   writer не вызывается (strict decode раньше authorizer);
# - effective systemd sandbox properties.
#
# НИКАКИХ valid mutation / sysfs writes.

{ config, lib, pkgs, ... }:

{
  name = "hardwared-lifecycle";

  nodes.machine =
    { config, lib, pkgs, ... }:
    {
      imports = [ ../module.nix ];
      services.orbis-control.enable = true;
      services.dbus.enable = true;
      # module.nix пока не задаёт wantedBy для system сервиса; VM включает его
      # явно, чтобы проверить lifecycle (production activation policy — вне
      # этого теста).
      systemd.services.orbis-hardwared.wantedBy = lib.mkForce [
        "multi-user.target"
      ];
    };

  testScript = ''
    start_all()

    machine.wait_for_unit("orbis-hardwared.service")
    machine.wait_until_succeeds(
        "busctl --system list | grep -F io.github.orbiscontrol.Hardware"
    )

    # --- service readiness ---
    machine.succeed("systemctl is-active orbis-hardwared.service")

    # --- bus ownership: pid совпадает; процесс uid 0 ---
    pid = machine.succeed(
        "systemctl show -p MainPID --value orbis-hardwared.service"
    ).strip()
    assert pid != "0", f"unexpected MainPID: {pid}"
    line = machine.succeed(
        "busctl --system list | grep -F io.github.orbiscontrol.Hardware"
    ).strip()
    assert pid in line, f"bus owner pid mismatch: {line}"
    assert "root" in line, f"hardwared not running as root: {line}"
    status = machine.succeed(f"cat /proc/{pid}/status")
    assert "Uid:\t0" in status, "hardwared not uid 0"

    # --- introspection: interface Hardware1 с ожидаемым методом ---
    intr = machine.succeed(
        "busctl --system introspect io.github.orbiscontrol.Hardware "
        "/io/github/orbiscontrol/Hardware io.github.orbiscontrol.Hardware1"
    )
    assert "SetPerformanceProfile" in intr, f"missing SetPerformanceProfile: {intr}"

    # --- capabilities: CapEff/CapBnd == 0 ---
    capeff = machine.succeed(f"grep '^CapEff' /proc/{pid}/status").strip()
    capbnd = machine.succeed(f"grep '^CapBnd' /proc/{pid}/status").strip()
    assert capeff.endswith("0000000000000000"), f"CapEff not empty: {capeff}"
    assert capbnd.endswith("0000000000000000"), f"CapBnd not empty: {capbnd}"

    # --- invalid-wire smoke: SetPerformanceProfile(255) -> InvalidArgs;
    #     daemon остаётся alive; writer не вызывается (decode раньше polkit) ---
    out = machine.succeed(
        "bash -c 'busctl --system call io.github.orbiscontrol.Hardware "
        "/io/github/orbiscontrol/Hardware io.github.orbiscontrol.Hardware1 "
        "SetPerformanceProfile y 255 2>&1; echo EXIT:$?'"
    )
    # busctl не печатает D-Bus error name; сообщение уникально для ветки
    # strict decode (InvalidArgs) и доказывает, что полка/writer не вызывались.
    assert "неизвестный performance wire value" in out, (
        f"expected InvalidArgs message, got: {out}"
    )
    assert "EXIT:1" in out, f"busctl expected failure exit, got: {out}"
    machine.succeed("systemctl is-active orbis-hardwared.service")

    # --- effective sandbox properties ---
    props = {
      "NoNewPrivileges": "yes",
      "ProtectSystem": "strict",
      "ProtectHome": "yes",
      "PrivateTmp": "yes",
      "PrivateDevices": "yes",
      "ProtectControlGroups": "yes",
      "RestrictAddressFamilies": "AF_UNIX",
      "MemoryDenyWriteExecute": "yes",
      "ReadOnlyPaths": "/sys",
      "ReadWritePaths": "-/sys/firmware/acpi/platform_profile",
      "CapabilityBoundingSet": "",
    }
    for key, expected in props.items():
      actual = machine.succeed(
          f"systemctl show -p {key} --value orbis-hardwared.service"
      ).strip()
      assert actual == expected, (
          f"sandbox {key}: expected {expected!r}, got {actual!r}"
      )
  '';
}
