{ lib
, rustPlatform
, pkg-config
, fontconfig
, freetype
, libGL
, xkbcommon
, wayland
, wayland-protocols
, dbus
, dbus-daemon
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
  ];

  buildInputs = [
    fontconfig
    freetype
    libGL
    xkbcommon
    wayland
    wayland-protocols
    dbus
    dbus-daemon
    openssl
    systemd
    glib
    cairo
    pango
    gdk-pixbuf
  ];

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
