# orbis-hardwared — standalone package для быстрого dev-deploy.
#
# В отличие от полного orbis-control, этот пакет:
# - собирает ТОЛЬКО orbis-hardwared (нет GUI/Slint/wayland deps)
# - НЕ запускает тесты (doCheck=false) — быстрая сборка
# - не оборачивает в LD_LIBRARY_PATH (нет dlopen в hardwared)
#
# Использование:
#   nix build .#orbis-hardwared --max-jobs 1 --cores 4

{ lib
, rustPlatform
, pkg-config
, dbus
, systemd
, ...
}:

rustPlatform.buildRustPackage {
  pname = "orbis-hardwared";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = lib.cleanSource ../..;
    filter = path: type:
      let
        root = toString ../..;
        rel =
          if lib.hasPrefix (root + "/") (toString path)
          then lib.removePrefix (root + "/") (toString path)
          else "";
        top = builtins.head (lib.splitString "/" rel);
      in
        rel == ""
        || builtins.elem top [ "crates" ]
        || builtins.elem rel [ "Cargo.toml" "Cargo.lock" "clippy.toml" "rustfmt.toml" ];
  };

  cargoLock = {
    lockFile = ../../Cargo.lock;
  };

  cargoBuildFlags = [ "-p" "orbis-hardwared" ];

  nativeBuildInputs = [ pkg-config ];

  buildInputs = [ dbus systemd ];

  # Быстрая dev-build: без тестов, без GUI wrapper.
  doCheck = false;

  postInstall = ''
    # D-Bus system policy (стабильная копия, не зависит от nix store path)
    install -Dm644 ${./dbus/io.github.orbiscontrol.Hardware.conf} \
      $out/share/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf

    # Polkit actions
    install -Dm644 ${./polkit/io.github.orbiscontrol.hardware.policy} \
      $out/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy
  '';

  meta = with lib; {
    description = "Orbis Control hardware helper (standalone, fast dev build)";
    license = licenses.gpl3Plus;
    platforms = platforms.linux;
  };
}
