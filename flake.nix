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
    }
    // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = nixpkgs.lib;
        orbis-control = pkgs.callPackage ./packaging/nix/package.nix { };
      in
      {
        # nix build .#orbis-control
        packages.default = orbis-control;
        packages.orbis-control = orbis-control;

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
        checks.default = pkgs.stdenv.mkDerivation {
          name = "orbis-control-flake-check";
          src = self;
          buildInputs = with pkgs; [ cargo rustc rustfmt clippy pkg-config dbus ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          buildPhase = ''
            export CARGO_HOME=$TMPDIR/cargo
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
        };
      });
}
