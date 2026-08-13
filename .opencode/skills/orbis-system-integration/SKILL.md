---
name: orbis-system-integration
description: Use for Orbis D-Bus, sessiond/client/protocol, hardwared, systemd, polkit, Nix packaging/module, or VM integration work.
---

# Orbis system integration

## Boundaries

- Session protocol, client, daemon и application остаются отдельными
  responsibilities.
- D-Bus DTOs являются untrusted wire input и валидируются на boundary.
- UI/domain не получают direct system/hardware I/O.
- Не использовать real system/session bus в tests без explicit task permission.
- Для D-Bus integration tests по умолчанию использовать существующий private
  P2P transport.
- Не вводить privileged path только ради удобства реализации.

## Nix / systemd / polkit

При изменениях packaging/nix, NixOS module, service definitions, polkit или
daemon bootstrap проверять integration path целиком, а не только изменённый
файл. Declarative source является authoritative; generated system files не
редактировать.

## Verification

- Выбирать существующую targeted проверку, которая наблюдает изменённую
  boundary.
- Для system integration использовать соответствующий existing VM check, а не
  запускать все VM checks механически.
- Exact Cargo/Nix commands и resource limits брать из project AGENTS.md.
- VM/fake-sysfs evidence не является доказательством real ASUS hardware
  behavior.
- Если затронута реальная hardware semantics, также загрузить
  `orbis-hardware-safety`.
- Для consequential integration work также использовать global
  `verify-important`.
