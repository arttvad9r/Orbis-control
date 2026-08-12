{ lib
, rustPlatform
, pkg-config
, makeWrapper
, fontconfig
, freetype
, libGL
, libglvnd
, libxkbcommon
, wayland
, wayland-protocols
, dbus
, openssl
, systemd
, glib
, cairo
, pango
, gdk-pixbuf
, ...
}:

rustPlatform.buildRustPackage {
  pname = "orbis-control";
  version = "0.1.0";

  # Source filter: только реальные build/test inputs для Rust-пакета, чтобы
  # docs/README и прочие файлы не инвалидировали derivation.
  #
  # Включены (проверено):
  # - Cargo.toml, Cargo.lock
  # - crates/** (Rust source + build.rs)
  # - ui/** (Slint sources; orbis-ui/build.rs компилирует ui/app-window.slint)
  # - tests/** (fixtures/dbus читаются тестами при doCheck)
  # - clippy.toml, rustfmt.toml (конфиги проверок пакета)
  #
  # Исключены: docs/**, README.md, data/**, packaging/**, tools/**,
  # flake.nix, flake.lock, LICENSE, .github/**, .opencode/**, deny.toml
  # и прочее, не используемое сборкой.
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
        rel == "" # корень
        || builtins.elem top [ "crates" "ui" "tests" ]
        || builtins.elem rel [ "Cargo.toml" "Cargo.lock" "clippy.toml" "rustfmt.toml" ];
  };

  cargoLock = {
    lockFile = ../../Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];

  buildInputs = [
    fontconfig
    freetype
    libGL
    libxkbcommon
    wayland
    wayland-protocols
    dbus
    openssl
    systemd
    glib
    cairo
    pango
    gdk-pixbuf
  ];

  # GUI (orbis-control) использует winit/glutin, которые загружают системные
  # библиотеки через dlopen (libwayland-client/cursor/egl, libxkbcommon,
  # libfontconfig, libEGL), поэтому они не видны в DT_NEEDED и не попадают в
  # RUNPATH пакета. Без wrapper packaged GUI падал при старте с
  # "The wayland library could not be loaded".
  #
  # Обёртка добавляет минимальный доказанный runtime library path только для
  # GUI. sessiond/ctl не требуют этих dlopen-зависимостей и не оборачиваются.
  postInstall = ''
    wrapProgram $out/bin/orbis-control \
      --prefix LD_LIBRARY_PATH : "${
        lib.makeLibraryPath [
          wayland
          libxkbcommon
          fontconfig
          libglvnd
        ]
      }"

    # D-Bus system policy: только root own + send_destination к hardwared
    # (авторизация операции — polkit внутри hardwared). Кладём в
    # share/dbus-1/system.d: NixOS dbus-daemon читает includedir
    # system-path/share/dbus-1/system.d (см. system.conf).
    install -Dm644 ${./dbus/io.github.orbiscontrol.Hardware.conf} \
      $out/share/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf

    # Polkit actions: Performance и Battery mutation.
    install -Dm644 ${./polkit/io.github.orbiscontrol.hardware.policy} \
      $out/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy
  '';

  # Запускаем полный набор проверок как часть пакета (как в CI).
  doCheck = true;

  meta = with lib; {
    description = "G-Helper-подобное приложение для ASUS-ноутбуков на Linux (нативный, Slint)";
    homepage = "https://github.com/orbis-control/orbis-control";
    license = licenses.gpl3Plus;
    platforms = platforms.linux;
    maintainers = with maintainers; [ ];
  };
}
