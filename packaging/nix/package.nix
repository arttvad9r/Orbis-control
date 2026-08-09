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

  src = lib.cleanSource ../..;

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
