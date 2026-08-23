# Изолированный NixOS VM test: production lifecycle `orbis-hardwared`
# БЕЗ hardware mutation.
#
# Проверяет:
# - system service `orbis-hardwared` существует и становится active;
# - system bus name `io.github.orbiscontrol.Hardware` owned процессом hardwared
#   (uid 0), Hardware1 introspection содержит обязательный typed
#   `SetPerformanceProfile` контракт;
# - CapEff/CapBnd == 0;
# - invalid-wire smoke не доходит до authorizer/writer;
# - effective systemd sandbox properties;
# - direct sysfs write surface содержит только production-enabled platform_profile;
# - packaged polkit allow_active=yes остаётся только у Performance/Battery.
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
    };

  testScript = ''
    import xml.etree.ElementTree as ET

    start_all()

    machine.wait_for_unit("orbis-hardwared.service")
    machine.wait_until_succeeds(
        "busctl --system list | grep -F io.github.orbiscontrol.Hardware"
    )

    machine.succeed("systemctl is-active orbis-hardwared.service")

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

    intr = machine.succeed(
        "busctl --system introspect io.github.orbiscontrol.Hardware "
        "/io/github/orbiscontrol/Hardware io.github.orbiscontrol.Hardware1"
    )
    assert "SetPerformanceProfile" in intr, f"missing SetPerformanceProfile: {intr}"

    capeff = machine.succeed(f"grep '^CapEff' /proc/{pid}/status").strip()
    capbnd = machine.succeed(f"grep '^CapBnd' /proc/{pid}/status").strip()
    assert capeff.endswith("0000000000000000"), f"CapEff not empty: {capeff}"
    assert capbnd.endswith("0000000000000000"), f"CapBnd not empty: {capbnd}"

    out = machine.succeed(
        "bash -c 'busctl --system call io.github.orbiscontrol.Hardware "
        "/io/github/orbiscontrol/Hardware io.github.orbiscontrol.Hardware1 "
        "SetPerformanceProfile y 255 2>&1; echo EXIT:$?'"
    )
    assert "неизвестный performance wire value" in out, (
        f"expected InvalidArgs message, got: {out}"
    )
    assert "EXIT:1" in out, f"busctl expected failure exit, got: {out}"
    machine.succeed("systemctl is-active orbis-hardwared.service")

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

    read_write_paths = machine.succeed(
        "systemctl show -p ReadWritePaths --value orbis-hardwared.service"
    ).strip().split()
    assert read_write_paths == ["-/sys/firmware/acpi/platform_profile"], read_write_paths
    assert "-/sys/class/leds/asus::kbd_backlight/brightness" not in read_write_paths
    assert "-/sys/class/leds/asus::kbd_backlight/max_brightness" not in read_write_paths
    assert "-/sys/class/leds" not in read_write_paths

    policy_text = machine.succeed(
        "cat /etc/polkit-1/actions/io.github.orbiscontrol.hardware.policy"
    )
    policy = ET.fromstring(policy_text)
    active_defaults = {
        action.attrib["id"]: action.findtext("./defaults/allow_active")
        for action in policy.findall("action")
    }
    expected_active = {
        "io.github.orbiscontrol.hardware.set-performance-profile": "yes",
        "io.github.orbiscontrol.hardware.set-charge-limit": "yes",
        "io.github.orbiscontrol.hardware.set-gpu-mode": "no",
        "io.github.orbiscontrol.hardware.set-product-gpu-mode": "no",
        "io.github.orbiscontrol.hardware.set-fan-curve": "no",
        "io.github.orbiscontrol.hardware.set-panel-overdrive": "no",
        "io.github.orbiscontrol.hardware.set-keyboard-backlight": "no",
        "io.github.orbiscontrol.hardware.set-aura-static-rgb": "no",
    }
    assert active_defaults == expected_active, (
        f"unexpected Hardware1 polkit defaults: {active_defaults!r}"
    )
  '';
}
