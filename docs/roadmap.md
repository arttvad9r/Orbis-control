# Roadmap

> Роль: **FUTURE PLAN**. Актуальная очередность работ без календарных обещаний.
> Фактическое состояние и evidence — в [`current-state.md`](current-state.md).
> Обновлено: **2026-08-19**.

## Principles

1. Сначала safety blockers и green build/CI, затем новые функции.
2. Фактическая поддержка определяется typed probes и evidence, а не названием модели.
3. Read/write evidence хранится раздельно.
4. `Accepted` не считается подтверждённым текущим состоянием.
5. Live claims всегда revision-scoped и требуют отдельной проверки.
6. Неполный или конфликтующий control остаётся fail-closed в UI и policy до исправления контракта.

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

- GitHub Actions должен реально выполнить и успешно завершить `nix flake check` (#106);
- старые remote validation refs можно физически удалить только через доступный branch-delete/git интерфейс.

## Milestone 1 — Stable production capabilities

**Status: CORE READ PATHS STABLE; WRITE EVIDENCE HARDENING ACTIVE**

Revision-scoped validated areas:

- Battery read path.
- Battery controlled setting path with confirmation.
- Performance read path.
- Performance controlled setting path with confirmation.
- Independent GPU power / MUX / access observations.
- Narrow typed system boundary.

Hardening still required:

- Battery/Panel/Aura mutation status must prove the actual write owner is reachable (#107), not merely that a backend object was constructed.
- Battery discovery must preserve permission/transient failures instead of collapsing them to structural Unsupported (#108).

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

**Status: PARTIAL / FAIL-CLOSED WHERE UNWIRED**

Completed:

- versioned `preferences.toml`;
- durable atomic writes and permission preservation;
- persisted Dark/Light theme;
- persisted Start Minimized;
- redacted config warnings;
- independent XDG window-state store;
- XDG autostart backend and Slint user-only toggle contract;
- main-window fake startup toggle removed;
- desktop/AppStream metadata and Nix installation wiring.

Next:

- finish the remaining Rust lifecycle glue for Run on Startup (#57); until then the Preferences toggle stays disabled;
- keep automation policy and desired hardware state separate from UI preferences;
- repeat packaged metadata validation on the final release revision.

## Milestone 4 — Fans and telemetry

**Status: READS IMPLEMENTED/TESTED; WRITES BLOCKED**

Telemetry provider and worker-owned polling are present in `main`.

Fan reads in `main` include active/profile-specific curves and lossless profile identity. Fan mutation/reset code exists but is **not currently an accepted write path**. The FansWindow mutation controls and packaged default fan polkit authorization are fail-closed.

Mandatory before any fan write is re-enabled:

1. Fix custom curve `CurveData.enabled` preservation and verify post-write enabled state (#104).
2. Make Factory Defaults profile restoration failure-safe (#105).
3. Fix FanCurves capability granularity so CPU support cannot imply GPU support (#109).
4. Obtain executable green tests/CI for the exact revision.
5. Perform dated controlled hardware validation and confirm final fan/profile state against authoritative backends.

## Milestone 5 — Diagnostics and supportability

**Status: TESTED FOUNDATIONS; LIFECYCLE WIRING PARTIAL / FAIL-CLOSED**

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

- finish Diagnostics window open/refresh lifecycle glue in current `main.rs` (#101); until then Refresh stays disabled and the window reports unavailable;
- keep export collection strictly allowlisted and privacy-bounded.

## Milestone 6 — GPU product policy

**Status: BLOCKED**

Eco / Standard / Ultimate / Optimized remain product-level policy rather than aliases for one low-level observation. Production raw GPU mutation remains disabled. Keep product controls unsupported/disabled until mapping, pending requirements, ownership and confirmation semantics are proven and tested.

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
- no known unsafe write path enabled by default;
- relevant Cargo integration checks green;
- `nix flake check` executed and green;
- package/metadata acceptance complete;
- Draft branches not counted as integrated behavior;
- dated evidence for device-specific claims;
- unsupported or incomplete controls shown honestly as unavailable/unknown.