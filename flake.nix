{
  description = "Orbis Control — G-Helper-подобное приложение для ASUS-ноутбуков на Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    {
      nixosModules.orbis-control = import ./packaging/nix/module.nix;

      # Static D-Bus/polkit registration only; standalone hardwared lifecycle
      # remains owned by packaging/deploy-dev-hardwared.sh.
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
        packages.default = orbis-control;
        packages.orbis-control = orbis-control;
        packages.orbis-hardwared = orbis-hardwared;
        packages.orbis-hardwared-policies = orbis-hardwared-policies;

        apps.default = {
          type = "app";
          program = "${orbis-control}/bin/orbis-control";
        };

        devShells.default = pkgs.mkShell {
          name = "orbis-control-dev";
          inputsFrom = [ orbis-control ];
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            rust-analyzer
            slint-lsp
            pkg-config
            dbus
            cargo-deny
            cargo-audit
            cargo-machete
            check-jsonschema
            dejavu_fonts
          ];
          QT_XKB_CONFIG_ROOT = "${pkgs.xkeyboard_config}/share/X11/xkb";
          XDG_DATA_DIRS = "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}";
        };

        # Canonical Rust workspace check. The package derivation already vendors
        # Cargo.lock dependencies; every Cargo command must additionally accept
        # the committed lockfile unchanged.
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
            cargo fmt --all -- --check
            cargo clippy --locked --workspace --all-targets -- -D warnings
            cargo test --locked --workspace
          '';
          installPhase = ''
            mkdir -p $out
            echo "flake check OK" > $out/check.log
          '';
          doCheck = false;
        });

        # Versioned support evidence must remain schema-valid. This uses the
        # flake-pinned nixpkgs tool rather than resolving a separate nix shell.
        checks.support-matrix-schema = pkgs.runCommand "orbis-support-matrix-schema" {
          nativeBuildInputs = [ pkgs.check-jsonschema ];
        } ''
          check-jsonschema \
            --schemafile ${./docs/support-matrix.schema.json} \
            ${./docs/support-matrix.examples}/*.json
          touch $out
        '';

        checks.hardwared-lifecycle = lib.nixos.runTest {
          hostPkgs = pkgs;
          imports = [ ./packaging/nix/tests/hardwared-lifecycle.nix ];
        };

        checks.performance-mutation-vm = lib.nixos.runTest {
          hostPkgs = pkgs;
          imports = [ ./packaging/nix/tests/performance-mutation-vm.nix ];
        };

        checks.battery-mutation-vm = lib.nixos.runTest {
          hostPkgs = pkgs;
          imports = [ ./packaging/nix/tests/battery-mutation-vm.nix ];
        };
      });
}
