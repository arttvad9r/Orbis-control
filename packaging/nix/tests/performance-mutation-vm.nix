{ config, lib, pkgs, ... }:

let
  fakeProfile = "/var/lib/orbis-control-test/platform_profile";
  fakeChoices = "/var/lib/orbis-control-test/platform_profile_choices";
  profilePath = "/sys/firmware/acpi/platform_profile";
  choicesPath = "/sys/firmware/acpi/platform_profile_choices";
  testRunner = pkgs.writeShellScriptBin "orbis-control-performance-mutation-test" ''
    set -eu
    result=/run/orbis-control-test/result
    log=/run/orbis-control-test/runner.log
    identity=/run/orbis-control-test/identity
    # Getty autologin respawns this script for every login session. Once one
    # generation completed the scenario, later generations must idle instead
    # of replaying mutations, otherwise a fresh generation races the driver's
    # post-PASS observations (observed: final profile read as 'quiet').
    complete=/run/orbis-control-test/scenario-complete
    if [[ -e $complete ]]; then
      exec sleep infinity
    fi
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

    # Performance must work with UPower intentionally absent. Battery discovery
    # is lazy/capability-local and must not block Session1 startup.
    ! busctl --system list | grep -F org.freedesktop.UPower

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
    # Mark completion before publishing PASS: the sentinel is the retry gate
    # for later autologin generations, so only a fully passed run sets it.
    : > "$complete"
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
      services.upower.enable = false;
      services.dbus.enable = true;
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

      environment.systemPackages = [ pkgs.gnugrep pkgs.procps pkgs.systemd ];

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

    machine.fail("busctl --system list | grep -F org.freedesktop.UPower")
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
    profile = machine.succeed(f"cat {profile_file}").strip()
    machine.log(f"final platform_profile={profile!r}")
    assert profile == "balanced", profile

    # Both daemons remain alive after all valid and invalid calls.
    machine.succeed("systemctl is-active orbis-hardwared.service")
    machine.succeed("loginctl list-sessions | grep -F orbis-test")
  '';
}