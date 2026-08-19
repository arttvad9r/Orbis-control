# Roadmap

> Роль: **FUTURE PLAN**. Актуальная очередность работ без календарных обещаний.
> Фактическое состояние и evidence — в [`current-state.md`](current-state.md).
> Обновлено: **2026-08-19**.

## Principles

1. Сначала green build/CI и одна понятная интеграционная линия.
2. Фактическая поддержка определяется typed probes и evidence, а не названием модели.
3. Read/write evidence хранится раздельно.
4. `Accepted` не считается подтверждённым текущим состоянием.
5. Live claims всегда revision-scoped и требуют отдельной проверки.

## Milestone 0 — Repository baseline

**Status: COMPLETED for consolidation; release CI remains BLOCKED**

Выполнено:

- production hardening объединён с `main`;
- Rust toolchain приведён к 1.87;
- startup resilience для Battery/UPower интегрирован;
- preferences, window-state и desired-state foundations интегрированы;
- Desired/Observed/Pending и lifecycle domain foundations интегрированы;
- diagnostics backend/export/runtime/window-model layers интегрированы;
- desktop/AppStream packaging и support-matrix tooling интегрированы;
- historical visual/theme PRs подтверждены как ancestors `main` и закрыты;
- source-of-truth документация обновлена;
- одноразовые validation/audit PR закрыты как integrated/superseded.

Остаётся:

- GitHub Actions должен реально выполнить и успешно завершить `nix flake check`;
- старые remote validation refs можно физически удалить только через доступный branch-delete/git интерфейс.

## Milestone 1 — Stable production capabilities

**Status: COMPLETED with revision-scoped live evidence**

- Battery read path.
- Battery controlled setting path with confirmation.
- Performance read path.
- Performance controlled setting path with confirmation.
- Independent GPU power / MUX / access observations.
- Narrow typed system boundary.

Эти результаты не являются универсальной таблицей поддержки всех ASUS моделей.

## Milestone 2 — Lifecycle and desired state

**Status: FOUNDATIONS INTEGRATED; reconciliation NOT IMPLEMENTED**

Completed:

- Session1 no longer depends on Battery availability at startup.
- Independent capabilities remain available when UPower is absent/unready.
- Desired / Observed / Pending typed state is in `main`.
- Inert Startup / Resume / BackendRecovered / CapabilityChanged events are in `main`.
- Generic versioned `desired-state.toml` persistence is in `main` and does not automatically apply anything.

Next:

- define reconciliation policy separately;
- compare authoritative observed state before proposing any action;
- preserve `Accepted != Applied` and explicit pending semantics;
- add integration tests before connecting lifecycle events to execution.

## Milestone 3 — Preferences and desktop integration

**Status: PARTIAL**

Completed:

- versioned `preferences.toml`;
- durable atomic writes and permission preservation;
- persisted Dark/Light theme;
- persisted Start Minimized;
- redacted config warnings;
- independent XDG window-state store;
- XDG autostart backend and Slint user-only toggle contract;
- desktop/AppStream metadata and Nix installation wiring.

Next:

- finish the remaining Rust lifecycle glue for Run on Startup (#57);
- keep automation policy and desired hardware state separate from UI preferences;
- repeat packaged metadata validation on the final release revision.

## Milestone 4 — Fans and telemetry

**Status: IMPLEMENTED/TESTED; acceptance incomplete**

Telemetry provider and worker-owned polling are present in `main`.

Fan support in `main` includes active/profile-specific reads, lossless profile identity, the visual editor and typed control/reset paths. Before promoting additional mutation/reset claims on the current revision, perform dated live validation and confirm final state against the authoritative backend.

## Milestone 5 — Diagnostics and supportability

**Status: TESTED foundations; lifecycle wiring PARTIAL**

Integrated in `main`:

- typed diagnostics domain;
- privacy-safe metadata sources;
- service/capability/GPU/telemetry/display observations;
- application collector;
- UI DTO;
- allowlisted versioned text/JSON exporters;
- end-to-end pure-data regression;
- read-only production DiagnosticsRuntime;
- pure DiagnosticsWindowModel;
- typed read-only DiagnosticsWindow Slint surface.

Next:

- finish Diagnostics window open/refresh lifecycle glue in current `main.rs` (#101);
- keep export collection strictly allowlisted and privacy-bounded.

## Milestone 6 — GPU product policy

**Status: BLOCKED**

Eco / Standard / Ultimate / Optimized remain product-level policy rather than aliases for one low-level observation. Keep them unsupported/disabled until mapping, pending requirements, ownership and confirmation semantics are proven and tested.

## Milestone 7 — Power limits and extended ASUS controls

**Status: BLOCKED / UNKNOWN by concept**

Typed scaffolding exists for some areas, but production evidence remains incomplete. Follow the dedicated readiness documents and add support concept-by-concept; do not infer support from DMI model names.

## Milestone 8 — CLI and release packaging

**Status: PARTIAL**

Completed:

- desktop/AppStream metadata and Nix installation wiring;
- release evidence taxonomy;
- support-matrix schema and validation tooling.

Remaining:

- replace the `orbisctl` stub with a real read/diagnostic CLI;
- obtain a green executable CI run on final `main`;
- perform final package/metadata acceptance on the release revision.

## Release gate

A beta/release candidate requires:

- intended baseline in `main`;
- relevant Cargo integration checks green;
- `nix flake check` executed and green;
- package/metadata acceptance complete;
- Draft branches not counted as integrated behavior;
- dated evidence for device-specific claims;
- unsupported controls shown honestly as unavailable/unknown.
