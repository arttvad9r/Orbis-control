# Orbis Control release metadata contract

> Роль: **CURRENT PACKAGING DESIGN**. Фактический package/release status — в
> [`current-state.md`](current-state.md). Обновлено: 2026-08-19.

## Canonical application identity

Use one reverse-DNS identity everywhere:

- application/component ID: `io.github.orbiscontrol.Orbis`;
- desktop file: `io.github.orbiscontrol.Orbis.desktop`;
- executable: `orbis-control`;
- XDG autostart owned filename: `io.github.orbiscontrol.Orbis.desktop`.

Current canonical repository/homepage is
`https://github.com/arttvad9r/Orbis-control`.

## Current packaged desktop entry

Installed path:

```text
$out/share/applications/io.github.orbiscontrol.Orbis.desktop
```

Current source intentionally uses:

- `Type=Application`;
- `Name=Orbis Control`;
- `GenericName=ASUS Laptop Control Center`;
- capability-driven comment/keywords;
- `TryExec=orbis-control` and `Exec=orbis-control`;
- `Terminal=false`;
- `Settings;HardwareSettings;` categories.

It intentionally does **not** claim:

- `DBusActivatable=true` — no application-activation contract exists;
- `PrefersNonDefaultGPU=true` — Orbis must not wake/request the dGPU merely to launch;
- `StartupWMClass` / `StartupNotify` without verified Slint/winit behavior;
- an application icon that is not actually packaged.

A future icon must use one project-owned name (recommended
`io.github.orbiscontrol.Orbis`), be installed into the hicolor hierarchy and be
validated before adding `Icon=` to the desktop entry. Do not use ASUS branding in
a way that implies affiliation.

## Current packaged AppStream metadata

Installed path:

```text
$out/share/metainfo/io.github.orbiscontrol.Orbis.metainfo.xml
```

Current metadata uses:

- component type `desktop-application`;
- component ID `io.github.orbiscontrol.Orbis`;
- project license `GPL-3.0-or-later`;
- matching desktop launchable ID;
- capability-driven description rather than universal ASUS support claims;
- canonical homepage URL.

Do not add release entries for unreleased versions/dates. Screenshots must come
from real packaged UI artifacts and must not imply hardware support that the
current capability/evidence state does not establish.

## Nix packaging state

`packaging/nix/package.nix` currently includes `data/**` in its source filter and
installs both desktop and AppStream files during `postInstall`. It also installs
the Hardware1 D-Bus policy and per-capability polkit policy.

This means metadata source files are not merely design assets: they are part of
the current package. Any metadata change therefore requires package-level
validation when executable CI is available.

## Validation contract

Before release metadata is accepted on a release revision:

1. validate desktop syntax with `desktop-file-validate`;
2. validate AppStream metadata with `appstreamcli validate --pedantic` or the repository's pinned equivalent;
3. verify AppStream launchable ID exactly matches the installed desktop filename;
4. verify `TryExec`/`Exec=orbis-control` resolves inside the built package;
5. if an icon is added, verify the named hicolor asset resolves in the built package;
6. launch the packaged GUI as a normal user without hardware writes;
7. verify package output actually contains the desktop/AppStream files;
8. keep PACKAGED evidence separate from LIVE-VALIDATED hardware evidence.

Current release acceptance remains blocked by non-executing GitHub Actions
(#106), so prior targeted packaging checks are historical evidence, not a green
validation of the latest `main`.

## Rules

- Keep launcher `Exec` argument-free until there is a stable launcher CLI contract.
- `orbisctl` is not the desktop launcher and remains a separate read-only-first CLI work item (#119).
- Do not add D-Bus activation merely to improve desktop integration.
- Do not add product features to metadata until current-state/evidence supports them.
- Package metadata and workspace/Nix repository URLs must remain consistent with the canonical repository.