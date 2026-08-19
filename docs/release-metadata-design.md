# Orbis Control release metadata design

Scope: backlog item #80. This is a design/packaging contract only; it does not modify `package.nix` or `module.nix`.

## Canonical application identity

Use one reverse-DNS desktop/application identifier everywhere:

- application ID: `io.github.orbiscontrol.Orbis`
- desktop file ID: `io.github.orbiscontrol.Orbis.desktop`
- AppStream component ID: `io.github.orbiscontrol.Orbis`
- icon name: `io.github.orbiscontrol.Orbis`
- executable: `orbis-control`

This matches the already implemented user-autostart filename and avoids introducing a second launcher identity.

## Desktop entry

Install as:

`$out/share/applications/io.github.orbiscontrol.Orbis.desktop`

Required/selected fields:

```ini
[Desktop Entry]
Type=Application
Name=Orbis Control
GenericName=ASUS Laptop Control Center
Comment=Monitor and control supported ASUS laptop features on Linux
Exec=orbis-control
Icon=io.github.orbiscontrol.Orbis
Terminal=false
Categories=Settings;HardwareSettings;
Keywords=ASUS;laptop;hardware;performance;battery;fans;GPU;
```

Rules:

- Keep `Exec` argument-free until there is a stable documented launcher CLI contract.
- Do not set `DBusActivatable=true`: the GUI does not currently expose a matching application-activation D-Bus contract.
- Do not set `StartupNotify` or `StartupWMClass` until the actual Slint/winit behavior is verified.
- Do not set `PrefersNonDefaultGPU=true`; a control/monitoring application must not request the dGPU and accidentally defeat power-saving behavior.
- Do not use the reserved `TrayIcon` category merely because future tray integration may exist.
- Do not add `SingleMainWindow=true`; Orbis owns additional native windows.

## AppStream metadata

Prepare:

`$out/share/metainfo/io.github.orbiscontrol.Orbis.metainfo.xml`

Minimum product metadata should include:

- component type `desktop-application`;
- component ID `io.github.orbiscontrol.Orbis`;
- name `Orbis Control`;
- concise summary;
- metadata license suitable for redistribution;
- project license `GPL-3.0-or-later`;
- `<launchable type="desktop-id">io.github.orbiscontrol.Orbis.desktop</launchable>`;
- project homepage/source URL only when the canonical public repository URL is settled;
- categories consistent with the desktop entry;
- developer/project name;
- content rating declaration when release tooling requires it;
- release entries only for actual released versions/dates;
- screenshots only from real packaged UI artifacts, never mock/live-hardware claims disguised as release evidence.

Do not describe unsupported ASUS features as universally available. Product description must remain capability-driven and model-agnostic.

## Icons

Use the same icon name `io.github.orbiscontrol.Orbis` for all sizes. Preferred source is a scalable SVG plus rendered PNG sizes only if packaging/desktop tooling requires them.

Target install locations:

- `share/icons/hicolor/scalable/apps/io.github.orbiscontrol.Orbis.svg`
- optional raster variants under `share/icons/hicolor/<size>x<size>/apps/`.

The icon must not use ASUS trademarks/logos in a way that implies affiliation. Orbis Control is an independent project.

## Validation contract

Before packaging integration is considered complete:

1. validate desktop file syntax with `desktop-file-validate`;
2. validate AppStream metadata with `appstreamcli validate --pedantic` (or the repository's pinned equivalent);
3. verify desktop ID and AppStream launchable match exactly;
4. verify `Exec=orbis-control` resolves inside the built package;
5. verify the named icon resolves through the hicolor icon theme;
6. run a packaged launcher smoke test without root and without hardware writes;
7. keep packaged/runtime evidence separate from live-hardware evidence.

## Current repository blockers

The current Nix package source filter explicitly excludes `data/**`, and `postInstall` installs only the D-Bus system policy and polkit action file. Therefore adding desktop/AppStream/icon source assets alone will not make them appear in the built package.

Backlog item #81 may safely prepare the source assets in a separate branch, but actual installation must wait for a packaging slice that is explicitly allowed to modify `packaging/nix/package.nix`.

No D-Bus activation change is required for desktop integration at this stage.
