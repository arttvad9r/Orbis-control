{ config, lib, pkgs, ... }:

let
  fakeRoot = "/var/lib/orbis-control-test/battery";
  fakePowerSupply = "${fakeRoot}/BAT0";
  fakeConfigured = "${fakeRoot}/configured";
  fakeSetterCalls = "${fakeRoot}/setter-calls";
  fakeEffective = "${fakePowerSupply}/charge_control_end_threshold";
  effectivePath = "/sys/class/power_supply/BAT0";

  fakeAsusd = pkgs.writeText "orbis-control-test-fake-asusd.py" ''
    import dbus
    import dbus.service
    from dbus.mainloop.glib import DBusGMainLoop
    from gi.repository import GLib

    BUS = "xyz.ljones.Asusd"
    PATH = "/xyz/ljones"
    IFACE = "xyz.ljones.Platform"
    PROPERTIES = "org.freedesktop.DBus.Properties"
    configured_file = "${fakeConfigured}"
    calls_file = "${fakeSetterCalls}"

    def read_value():
        with open(configured_file) as handle:
            return int(handle.read().strip())

    def write_value(value):
        with open(configured_file, "w") as handle:
            handle.write(f"{value}\n")

    def increment_calls():
        with open(calls_file) as handle:
            calls = int(handle.read().strip())
        with open(calls_file, "w") as handle:
            handle.write(f"{calls + 1}\n")

    class Platform(dbus.service.Object):
        def __init__(self, bus, path):
            super().__init__(bus, path)

        @dbus.service.method(PROPERTIES, in_signature="ss", out_signature="v")
        def Get(self, interface, name):
            if interface != IFACE or name != "ChargeControlEndThreshold":
                raise dbus.exceptions.DBusException(
                    "unknown property", name="org.freedesktop.DBus.Error.InvalidArgs"
                )
            return dbus.Byte(read_value())

        @dbus.service.method(PROPERTIES, in_signature="s", out_signature="a{sv}")
        def GetAll(self, interface):
            if interface != IFACE:
                return {}
            return {"ChargeControlEndThreshold": dbus.Byte(read_value())}

        @dbus.service.method(PROPERTIES, in_signature="ssv", out_signature="")
        def Set(self, interface, name, value):
            if interface != IFACE or name != "ChargeControlEndThreshold":
                raise dbus.exceptions.DBusException(
                    "unknown property", name="org.freedesktop.DBus.Error.InvalidArgs"
                )
            value = int(value)
            increment_calls()
            if value == 77:
                raise dbus.exceptions.DBusException(
                    "simulated setter failure", name="org.freedesktop.DBus.Error.Failed"
                )
            write_value(value)

    DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    bus.request_name(BUS)
    Platform(bus, PATH)
    GLib.MainLoop().run()
  '';
  python = pkgs.python3.withPackages (ps: [ ps.dbus-python ps.pygobject3 ]);
  # Type=simple marks the unit "started" when the process spawns, but the bus
  # name is acquired only after python imports finish. Without this readiness
  # gate the hardwared one-shot owner preflight can lose that race, install no
  # mutation backend (fail-closed) and every SetChargeLimit fails until restart.
  fakeAsusdReady = pkgs.writeShellScript "orbis-control-test-fake-asusd-ready" ''
    for _ in $(seq 1 100); do
      if ${pkgs.systemd}/bin/busctl --system list \
          | ${pkgs.gnugrep}/bin/grep -Fq xyz.ljones.Asusd; then
        exit 0
      fi
      ${pkgs.coreutils}/bin/sleep 0.1
    done
    echo "fake asusd did not acquire xyz.ljones.Asusd within 10s" >&2
    exit 1
  '';
  fakeAsusdPolicy = pkgs.writeTextDir "share/dbus-1/system.d/orbis-control-test-asusd.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
      "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="xyz.ljones.Asusd"/>
        <allow send_destination="xyz.ljones.Asusd"/>
      </policy>
      <policy context="default">
        <allow send_destination="xyz.ljones.Asusd"/>
      </policy>
    </busconfig>
  '';
  testRunner = pkgs.writeShellScriptBin "orbis-control-battery-mutation-test" ''
    set -eu
    result=/run/orbis-control-test/battery-result
    log=/run/orbis-control-test/battery-runner.log
    exec >"$log" 2>&1

    test "$(id -u)" != 0
    test -n "$XDG_SESSION_ID"
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Type --value)" = tty
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Active --value)" = yes

    call() {
      busctl --system call \
        io.github.orbiscontrol.Hardware \
        /io/github/orbiscontrol/Hardware \
        io.github.orbiscontrol.Hardware1 SetChargeLimit y "$1"
    }

    if invalid=$(call 19 2>&1); then
      printf '19 unexpectedly succeeded: %s\n' "$invalid"
      exit 1
    fi
    printf '%s\n' "$invalid" | grep -E 'InvalidArgs|20..=100'
    test "$(cat ${fakeSetterCalls})" = 0

    if invalid=$(call 101 2>&1); then
      printf '101 unexpectedly succeeded: %s\n' "$invalid"
      exit 1
    fi
    printf '%s\n' "$invalid" | grep -E 'InvalidArgs|20..=100'
    test "$(cat ${fakeSetterCalls})" = 0

    call 80 | grep -E 'y 80$'
    test "$(cat ${fakeSetterCalls})" = 1
    test "$(cat ${fakeConfigured})" = 80
    test "$(cat ${fakeEffective})" = 100

    # configured=80/effective=100 is the permitted divergence case; the
    # returned value is the confirmed configured value.
    call 100 | grep -E 'y 100$'
    test "$(cat ${fakeSetterCalls})" = 2
    test "$(cat ${fakeConfigured})" = 100
    test "$(cat ${fakeEffective})" = 100

    if failed=$(call 77 2>&1); then
      printf 'setter failure unexpectedly succeeded: %s\n' "$failed"
      exit 1
    fi
    printf '%s\n' "$failed" | grep -E 'Failed|simulated setter failure'
    test "$(cat ${fakeSetterCalls})" = 3

    printf 'PASS\n' > "$result"
    cp "$log" /run/orbis-control-test/battery-result.log
  '';
in
{
  name = "battery-mutation-vm";

  nodes.machine =
    { config, lib, pkgs, ... }:
    {
      imports = [ ../module.nix ];
      services.orbis-control.enable = true;
      services.dbus.enable = true;
      services.dbus.packages = [ fakeAsusdPolicy ];
      security.polkit.enable = true;

      users.users.orbis-test = {
        isNormalUser = true;
        password = "orbis-test";
        linger = true;
        shell = "${testRunner}/bin/orbis-control-battery-mutation-test";
      };
      services.getty.autologinUser = "orbis-test";
      systemd.tmpfiles.rules = [
        "d ${fakeRoot} 0755 root root -"
        "d /run/orbis-control-test 0755 orbis-test users -"
      ];
      environment.systemPackages = [ pkgs.glib pkgs.gnugrep pkgs.procps pkgs.systemd ];

      systemd.services.orbis-control-test-fake-files = {
        wantedBy = [ "multi-user.target" ];
        before = [ "orbis-control-test-fake-asusd.service" "orbis-hardwared.service" ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = ''
          mkdir -p ${fakePowerSupply}
          printf '100\n' > ${fakeConfigured}
          printf '0\n' > ${fakeSetterCalls}
          printf 'Battery\n' > ${fakePowerSupply}/type
          printf '100\n' > ${fakeEffective}
          chmod 0644 ${fakeConfigured} ${fakeSetterCalls} ${fakePowerSupply}/type ${fakeEffective}
        '';
      };

      systemd.services.orbis-control-test-fake-asusd = {
        wantedBy = [ "multi-user.target" ];
        after = [ "dbus.service" "orbis-control-test-fake-files.service" ];
        requires = [ "dbus.service" "orbis-control-test-fake-files.service" ];
        serviceConfig = {
          Type = "simple";
          ExecStart = "${python}/bin/python ${fakeAsusd}";
          ExecStartPost = fakeAsusdReady;
          Restart = "on-failure";
        };
      };

      systemd.services.orbis-hardwared = {
        wantedBy = lib.mkForce [ "multi-user.target" ];
        requires = [ "orbis-control-test-fake-asusd.service" ];
        after = [ "orbis-control-test-fake-asusd.service" ];
        serviceConfig = {
          TemporaryFileSystem = [ "/sys/class/power_supply" ];
          BindReadOnlyPaths = [ "${fakePowerSupply}:${effectivePath}" ];
        };
      };
    };

  testScript = ''
    import re

    start_all()
    machine.wait_for_unit("orbis-control-test-fake-asusd.service")
    machine.wait_for_unit("orbis-hardwared.service")
    machine.wait_until_succeeds("busctl --system list | grep -F xyz.ljones.Asusd")
    machine.wait_until_succeeds("busctl --system list | grep -F io.github.orbiscontrol.Hardware")
    machine.succeed("systemctl is-active orbis-hardwared.service")

    pid = machine.succeed(
        "systemctl show -p MainPID --value orbis-hardwared.service"
    ).strip()
    assert pid != "0"
    status = machine.succeed(f"cat /proc/{pid}/status")
    assert "Uid:\t0" in status
    assert machine.succeed(f"grep '^CapEff' /proc/{pid}/status").strip().endswith(
        "0000000000000000"
    )
    assert machine.succeed(f"grep '^CapBnd' /proc/{pid}/status").strip().endswith(
        "0000000000000000"
    )

    intr = machine.succeed(
        "busctl --system introspect io.github.orbiscontrol.Hardware "
        "/io/github/orbiscontrol/Hardware io.github.orbiscontrol.Hardware1"
    )
    assert "SetPerformanceProfile" in intr
    assert "SetChargeLimit" in intr

    policy = machine.succeed(
        "pkaction --action-id io.github.orbiscontrol.hardware.set-charge-limit --verbose"
    )
    expected_policy = {"any": "no", "inactive": "no", "active": "yes"}
    parsed_policy = {
        name: value
        for name, value in re.findall(
            r"^\s*implicit (any|inactive|active):\s*(\S+)",
            policy,
            re.MULTILINE,
        )
    }
    machine.log("pkaction raw output:\n" + policy)
    machine.log("pkaction parsed values: " + repr(parsed_policy))
    machine.log("pkaction expected values: " + repr(expected_policy))
    assert parsed_policy == expected_policy, (
        f"pkaction policy mismatch: expected={expected_policy!r}, "
        f"actual={parsed_policy!r}\nraw={policy}"
    )

    machine.wait_until_succeeds("test -f /run/orbis-control-test/battery-result")
    result = machine.succeed("cat /run/orbis-control-test/battery-result")
    assert "PASS" in result
    machine.succeed("systemctl is-active orbis-control-test-fake-asusd.service")
    machine.succeed("systemctl is-active orbis-hardwared.service")
  '';
}
