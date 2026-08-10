{ config, lib, pkgs, ... }:

let
  fakeProfile = "/var/lib/orbis-control-test/platform_profile";
  fakeChoices = "/var/lib/orbis-control-test/platform_profile_choices";
  profilePath = "/sys/firmware/acpi/platform_profile";
  choicesPath = "/sys/firmware/acpi/platform_profile_choices";
  fakeUpower = pkgs.writeText "orbis-control-test-fake-upower.py" ''
    import dbus
    import dbus.service
    from dbus.mainloop.glib import DBusGMainLoop
    from gi.repository import GLib


    UPOWER = "org.freedesktop.UPower"
    DEVICE = "org.freedesktop.UPower.Device"
    BATTERY = "/org/freedesktop/UPower/devices/DisplayDevice"


    class Root(dbus.service.Object):
        @dbus.service.method(UPOWER, in_signature="", out_signature="ao")
        def EnumerateDevices(self):
            return [dbus.ObjectPath(BATTERY)]


    class Battery(dbus.service.Object):
        @dbus.service.method(
            "org.freedesktop.DBus.Properties",
            in_signature="ss",
            out_signature="v",
        )
        def Get(self, interface, name):
            if interface != DEVICE:
                raise dbus.exceptions.DBusException(
                    "unknown interface", name="org.freedesktop.DBus.Error.InvalidArgs"
                )
            values = {
                "Type": dbus.UInt32(2),
                "PowerSupply": dbus.Boolean(True),
                "ChargeThresholdSupported": dbus.Boolean(True),
                "ChargeThresholdEnabled": dbus.Boolean(True),
                "ChargeEndThreshold": dbus.UInt32(80),
            }
            if name not in values:
                raise dbus.exceptions.DBusException(
                    "unknown property", name="org.freedesktop.DBus.Error.InvalidArgs"
                )
            return values[name]

        @dbus.service.method(
            "org.freedesktop.DBus.Properties",
            in_signature="s",
            out_signature="a{sv}",
        )
        def GetAll(self, interface):
            if interface != DEVICE:
                return {}
            return {
                "Type": dbus.UInt32(2),
                "PowerSupply": dbus.Boolean(True),
                "ChargeThresholdSupported": dbus.Boolean(True),
                "ChargeThresholdEnabled": dbus.Boolean(True),
                "ChargeEndThreshold": dbus.UInt32(80),
            }


    DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    bus.request_name(UPOWER)
    Root(bus, "/org/freedesktop/UPower")
    Battery(bus, BATTERY)
    GLib.MainLoop().run()
  '';
  python = pkgs.python3.withPackages (ps: [ ps.dbus-python ps.pygobject3 ]);
  fakeUpowerPolicy = pkgs.writeTextDir "share/dbus-1/system.d/orbis-control-test-upower.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
      "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="org.freedesktop.UPower"/>
        <allow send_destination="org.freedesktop.UPower"/>
      </policy>
      <policy context="default">
        <allow send_destination="org.freedesktop.UPower"/>
      </policy>
    </busconfig>
  '';
  testRunner = pkgs.writeShellScriptBin "orbis-control-performance-mutation-test" ''
    set -eu
    result=/run/orbis-control-test/result
    log=/run/orbis-control-test/runner.log
    identity=/run/orbis-control-test/identity
    exec >"$log" 2>&1

    test "$(id -u)" != 0
    test -n "$XDG_SESSION_ID"
    {
      printf 'pid=%s\n' "$$"
      printf 'ppid=%s\n' "$PPID"
      id
      printf 'cgroup:\n'
      cat /proc/$$/cgroup
      printf 'session:\n'
      loginctl show-session "$XDG_SESSION_ID" \
        -p Class -p Type -p Active -p Remote -p Seat -p Leader
    } >"$identity"
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Type --value)" = tty
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Class --value)" = user
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Active --value)" = yes
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Remote --value)" = no
    test "$(loginctl show-session "$XDG_SESSION_ID" -p Seat --value)" = seat0

    systemctl --user start orbis-sessiond.service
    busctl --user status io.github.orbiscontrol.Session >/dev/null

    get_performance() {
      busctl --user get-property \
        io.github.orbiscontrol.Session \
        /io/github/orbiscontrol/Session \
        io.github.orbiscontrol.Session1 Performance | awk '{ print $2 }'
    }

    set_performance() {
      busctl --system call \
        io.github.orbiscontrol.Hardware \
        /io/github/orbiscontrol/Hardware \
        io.github.orbiscontrol.Hardware1 SetPerformanceProfile y "$1"
    }

    assert_state() {
      test "$(cat ${fakeProfile})" = "$1"
      test "$(get_performance)" = "$2"
    }

    test "$(cat ${fakeChoices})" = "quiet balanced performance"
    assert_state balanced 1

    set_performance 0 | grep -E 'y 0$'
    assert_state quiet 0
    set_performance 2 | grep -E 'y 2$'
    assert_state performance 2
    set_performance 1 | grep -E 'y 1$'
    assert_state balanced 1

    if invalid=$(set_performance 255 2>&1); then
      printf 'invalid call unexpectedly succeeded: %s\n' "$invalid"
      exit 1
    fi
    printf '%s\n' "$invalid" | grep -E 'InvalidArgs|unknown performance wire value|неизвестный performance wire value'
    assert_state balanced 1
    systemctl --user is-active orbis-sessiond.service
    printf 'PASS\n'
    cp "$log" "$result"
  '';
in
{
  name = "performance-mutation-vm";

  nodes.machine =
    { config, lib, pkgs, ... }:
    {
      imports = [ ../module.nix ];

      services.orbis-control.enable = true;
      services.dbus.enable = true;
      services.dbus.packages = [ fakeUpowerPolicy ];
      security.polkit.enable = true;

      # Непривилегированный пользователь получает настоящую PAM/logind
      # локальную активную сессию через getty autologin.
      users.users.orbis-test = {
        isNormalUser = true;
        password = "orbis-test";
        linger = true;
        shell = "${testRunner}/bin/orbis-control-performance-mutation-test";
      };
      services.getty.autologinUser = "orbis-test";
      systemd.tmpfiles.rules = [
        "d /run/orbis-control-test 0755 orbis-test users -"
      ];

      # Session1 запускается обычным user manager, созданным PAM/logind для
      # getty-сессии; отдельный test bus или system service здесь запрещены.
      systemd.user.services.orbis-sessiond.wantedBy = lib.mkForce [ "default.target" ];
      systemd.user.services.orbis-sessiond.serviceConfig = {
        TemporaryFileSystem = [ "/sys/firmware/acpi" ];
        BindReadOnlyPaths = [
          "${fakeProfile}:${profilePath}"
          "${fakeChoices}:${choicesPath}"
        ];
      };

      environment.etc."orbis-control-test/fake-upower.py" = {
        source = fakeUpower;
        mode = "0755";
      };
      environment.systemPackages = [ pkgs.glib pkgs.gnugrep pkgs.procps pkgs.systemd ];

      # Подготовить source files до создания hardwared namespace. Это только
      # VM fixture; production writer и его fixed paths не изменяются.
      systemd.services.orbis-control-test-fake-files = {
        description = "Orbis Control test fake platform_profile files";
        wantedBy = [ "multi-user.target" ];
        before = [ "orbis-hardwared.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          mkdir -p /var/lib/orbis-control-test
          printf 'balanced\n' > ${fakeProfile}
          printf 'quiet balanced performance\n' > ${fakeChoices}
          chmod 0644 ${fakeProfile} ${fakeChoices}
        '';
      };

      systemd.services.orbis-control-test-fake-upower = {
        description = "Orbis Control test fake UPower battery";
        wantedBy = [ "multi-user.target" ];
        after = [ "dbus.service" ];
        requires = [ "dbus.service" ];
        serviceConfig = {
          Type = "simple";
          ExecStart = "${python}/bin/python ${config.environment.etc."orbis-control-test/fake-upower.py".source}";
          Restart = "on-failure";
        };
      };

      # Test-only bind namespace. hardwared получает exact production paths;
      # choices монтируется только read-only. Sessiond получает те же fake
      # inodes read-only, чтобы его обычный Performance getter видел fresh
      # state, записанный hardwared.
      systemd.services.orbis-hardwared = {
        wantedBy = lib.mkForce [ "multi-user.target" ];
        requires = [ "orbis-control-test-fake-files.service" ];
        after = [ "orbis-control-test-fake-files.service" ];
        serviceConfig = {
          TemporaryFileSystem = [ "/sys/firmware/acpi" ];
          BindPaths = [ "${fakeProfile}:${profilePath}" ];
          BindReadOnlyPaths = [ "${fakeChoices}:${choicesPath}" ];
        };
      };

    };

  testScript = ''
    import re
    profile_file = "${fakeProfile}"
    choices_file = "${fakeChoices}"

    start_all()

    machine.wait_for_unit("orbis-control-test-fake-upower.service")
    machine.wait_until_succeeds(
        "busctl --system list | grep -F org.freedesktop.UPower"
    )
    machine.wait_for_unit("orbis-hardwared.service")
    machine.succeed("systemctl is-active orbis-hardwared.service")

    # Доказать, что policy использует active-user default, а не permissive
    # allow_any override. Сам успешный mutation ниже дополнительно доказывает
    # реальный polkit authorization path.
    policy = machine.succeed(
        "pkaction --action-id io.github.orbiscontrol.hardware.set-performance-profile --verbose"
    )
    assert re.search(r"implicit active:\s+yes", policy), policy
    assert re.search(r"implicit any:\s+no", policy), policy

    assert machine.succeed(f"cat {choices_file}").strip() == "quiet balanced performance"
    try:
        machine.wait_until_succeeds(
            "test -f /run/orbis-control-test/result", timeout=90
        )
    except Exception as error:
        machine.log(f"performance mutation runner timed out: {error}")
        for command in [
            "systemctl status getty@tty1.service --no-pager || true",
            "journalctl -b -u getty@tty1.service --no-pager || true",
            "journalctl -b _COMM=login --no-pager || true",
            "journalctl -b -u orbis-sessiond.service --no-pager || true",
            "journalctl -b -u orbis-hardwared.service --no-pager || true",
            "loginctl list-sessions || true",
            "cat /run/orbis-control-test/identity || true",
            "cat /run/orbis-control-test/runner.log || true",
        ]:
            status, output = machine.execute(command)
            machine.log(f"diagnostic {command} (exit {status}):\n{output}")
        raise
    result = machine.succeed("cat /run/orbis-control-test/result")
    assert "PASS" in result, result
    assert machine.succeed(f"cat {profile_file}").strip() == "balanced"

    # Both daemons remain alive after all valid and invalid calls.
    machine.succeed("systemctl is-active orbis-hardwared.service")
    machine.succeed("loginctl list-sessions | grep -F orbis-test")
  '';
}
