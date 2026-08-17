{
  description = "Orbis Control — G-Helper-подобное приложение для ASUS-ноутбуков на Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    {
      # NixOS module (system-independent): services.orbis-control
      nixosModules.orbis-control = import ./packaging/nix/module.nix;

      # Минимальный NixOS-модуль: только static D-Bus/polkit registration
      # для orbis-hardwared. НЕ создаёт systemd service, НЕ включает
      # services.orbis-control. Lifecycle демона — deploy-dev-hardwared.sh.
      nixosModules.orbis-hardwared-policies =
        import ./packaging/nix/hardwared-policies-module.nix;
    }
    // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = nixpkgs.lib;
        orbis-control = pkgs.callPackage ./packaging/nix/package.nix { };
        orbis-hardwared = pkgs.callPackage ./packaging/nix/hardwared.nix { };
        orbis-hardwared-policies = pkgs.callPackage ./packaging/nix/hardwared-policies.nix { };
      in
      {
        # nix build .#orbis-control
        packages.default = orbis-control;
        packages.orbis-control = orbis-control;
        # nix build .#orbis-hardwared  (fast standalone daemon)
        packages.orbis-hardwared = orbis-hardwared;
        # nix build .#orbis-hardwared-policies  (D-Bus + polkit only, no binary)
        packages.orbis-hardwared-policies = orbis-hardwared-policies;

        # nix run .#orbis-control -- --mock-device zephyrus-full
        apps.default = {
          type = "app";
          program = "${orbis-control}/bin/orbis-control";
        };

        # nix develop
        devShells.default = pkgs.mkShell {
          name = "orbis-control-dev";
          inputsFrom = [ orbis-control ];
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            # Language servers для editor/OpenCode LSP интеграции
            rust-analyzer
            slint-lsp
            pkg-config
            # D-Bus для integration-тестов (временная шина); dbus-daemon входит в pkgs.dbus
            dbus
            # Инструменты проверки
            cargo-deny
            cargo-audit
            cargo-machete
            # Шрифт для screenshot-тестов в CI
            dejavu_fonts
          ];
          # Окружение для headless-рендера Slint (MockRenderingBackend) и тестов
          QT_XKB_CONFIG_ROOT = "${pkgs.xkeyboard_config}/share/X11/xkb";
          XDG_DATA_DIRS = "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}";
        };

        # nix flake check
        # Переиспользует offline Cargo dependency machinery package
        # derivation (cargoLock vendoring) и реально выполняет fmt/clippy/test.
        checks.default = orbis-control.overrideAttrs (old: {
          pname = "orbis-control-flake-check";
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [
            pkgs.rustfmt
            pkgs.clippy
          ];
          buildPhase = ''
            export XDG_CACHE_HOME=$TMPDIR/cache
            export XDG_DATA_HOME=$TMPDIR/data
            export XDG_CONFIG_HOME=$TMPDIR/config
            cargo fmt --check
            cargo clippy --workspace --all-targets -- -D warnings
            cargo test --workspace
          '';
          installPhase = ''
            mkdir -p $out
            echo "flake check OK" > $out/check.log
          '';
          doCheck = false;
        });

        # Targeted NixOS VM test: production lifecycle orbis-hardwared
        # без hardware mutation (service/bus/caps/introspection/invalid-wire).
        checks.hardwared-lifecycle = lib.nixos.runTest {
          hostPkgs = pkgs;
          imports = [ ./packaging/nix/tests/hardwared-lifecycle.nix ];
        };

        # Targeted E2E Performance mutation through Session1 → hardwared,
        # using only a test-only bind namespace for fake platform_profile.
        checks.performance-mutation-vm = lib.nixos.runTest {
          hostPkgs = pkgs;
          imports = [ ./packaging/nix/tests/performance-mutation-vm.nix ];
        };

        # Targeted E2E Battery mutation through the original active local
        # caller, using only fake asusd and fake power-supply files in the VM.
        checks.battery-mutation-vm = lib.nixos.runTest {
          hostPkgs = pkgs;
          imports = [ ./packaging/nix/tests/battery-mutation-vm.nix ];
        };
      });
}
