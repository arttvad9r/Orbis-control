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

  # Source filter: только реальные build/test/package inputs, чтобы docs/README
  # и прочие файлы не инвалидировали derivation.
  #
  # Включены (проверено):
  # - Cargo.toml, Cargo.lock
  # - crates/** (Rust source + build.rs)
  # - ui/** (Slint sources; orbis-ui/build.rs компилирует ui/app-window.slint)
  # - tests/** (fixtures/dbus читаются тестами при doCheck)
  # - data/** (.desktop + AppStream metadata installed by postInstall)
  # - clippy.toml, rustfmt.toml (конфиги проверок пакета)
  #
  # Исключены: docs/**, README.md, packaging/** (кроме Nix packaging inputs), tools/**,
  # flake.nix, flake.lock, LICENSE, .github/**, .opencode/**, deny.toml
  # и прочее, не используемое сборкой/установкой.
  #
  # Policy and D-Bus files are package resources referenced below. The hardwared
  # policy test receives its Nix store resource path through preCheck instead of
  # assuming that ../../packaging survives source filtering.
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
        || builtins.elem top [ "crates" "ui" "tests" "data" ]
        || builtins.elem rel [ "Cargo.toml" "Cargo.lock" "clippy.toml" "rustfmt.toml" ]
        || lib.hasPrefix "packaging/nix/" rel;
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

  preCheck = ''
    export ORBIS_HARDWARED_POLKIT_POLICY="${./polkit/io.github.orbiscontrol.hardware.policy}"
  '';

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

    # Desktop/AppStream metadata is source-owned and installed only after the
    # metadata slice has been reviewed. No custom icon is claimed here.
    install -Dm644 data/applications/io.github.orbiscontrol.Orbis.desktop \
      $out/share/applications/io.github.orbiscontrol.Orbis.desktop
    install -Dm644 data/metainfo/io.github.orbiscontrol.Orbis.metainfo.xml \
      $out/share/metainfo/io.github.orbiscontrol.Orbis.metainfo.xml

    # D-Bus system policy: только root own + send_destination к hardwared
    # (авторизация операции — polkit внутри hardwared). Кладём в
    # share/dbus-1/system.d: NixOS dbus-daemon читает includedir
    # system-path/share/dbus-1/system.d (см. system.conf).
    install -Dm644 ${./dbus/io.github.orbiscontrol.Hardware.conf} \
      $out/share/dbus-1/system.d/io.github.orbiscontrol.Hardware.conf

    # Per-capability Hardware1 polkit actions. Individual defaults may be
    # fail-closed while a mutation contract is blocked (for example fan writes).
    install -Dm644 ${./polkit/io.github.orbiscontrol.hardware.policy} \
      $out/share/polkit-1/actions/io.github.orbiscontrol.hardware.policy
  '';

  # Запускаем полный набор проверок как часть пакета (как в CI).
  doCheck = true;

  meta = with lib; {
    description = "G-Helper-подобное приложение для ASUS-ноутбуков на Linux (нативный, Slint)";
    homepage = "https://github.com/arttvad9r/Orbis-control";
    license = licenses.gpl3Plus;
    platforms = platforms.linux;
    maintainers = with maintainers; [ ];
  };
}
