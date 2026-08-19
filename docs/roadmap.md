# Roadmap

> Роль: **FUTURE PLAN**. Актуальная очередность работ без календарных обещаний.
> Фактическое состояние и evidence — в [`current-state.md`](current-state.md).
> Обновлено: **2026-08-19**.

## Principles

1. Сначала safety blockers, runtime reliability и green build/CI, затем новые функции.
2. Фактическая поддержка определяется typed probes и evidence, а не названием модели.
3. Read/write evidence хранится раздельно.
4. `Accepted` не считается подтверждённым текущим состоянием.
5. Live claims всегда revision-scoped и требуют отдельной проверки.
6. Неполный или конфликтующий control остаётся fail-closed в UI/policy до исправления контракта.

## Milestone 0 — Repository baseline

**Status: CONSOLIDATED; release CI BLOCKED**

Completed:

- production hardening merged into `main`;
- Rust toolchain moved to 1.87;
- Battery/UPower startup resilience integrated;
- preferences, window-state, desired-state and lifecycle foundations integrated;
- diagnostics backend/export/runtime/window-model foundations integrated;
- desktop/AppStream packaging and support-matrix tooling integrated;
- obsolete PR stack normalized; open PR count is zero;
- current README/product/architecture/provider/security/current-state documentation normalized.

Remaining:

- restore executable GitHub Actions and obtain green current-main checks (#106);
- prune 139 obsolete/validation `agent/*` refs when branch-delete access exists (#118);
- after CI recovery, protect `main` with real required checks (#114);
- confirm permanent reverse-DNS application identity before stable release (#124).

## Milestone 1 — Stable production capabilities

**Status: CORE READ PATHS STABLE; WRITE EVIDENCE HARDENING ACTIVE**

Revision-scoped validated areas:

- Battery read path;
- controlled Battery setting path with confirmation;
- Performance read path;
- controlled Performance setting path with confirmation;
- independent GPU power / MUX / access observations;
- narrow typed Hardware1 system boundary.

Hardening still required:

- prove Battery/Panel/Aura actual mutation owners/interfaces are reachable before reporting write Supported (#107);
- preserve Battery discovery permission/transient failures instead of collapsing them to Unsupported (#108);
- explicit capability refresh must re-query mutation status before rebuilding the registry (#112);
- effective write evidence must include explicit product/policy safety blocks instead of reporting a denied fan path as writable (#120);
- enforce provider/status timeouts so one hung backend cannot stall the sequential worker and registry indefinitely (#123).

These claims are not a universal support table for all ASUS models.

## Milestone 2 — Lifecycle and desired state

**Status: FOUNDATIONS INTEGRATED; reconciliation NOT IMPLEMENTED**

Completed:

- Session1 no longer depends on Battery availability at startup;
- Desired / Observed / Pending typed state is in `main`;
- inert Startup / Resume / BackendRecovered / CapabilityChanged values are in `main`;
- generic versioned `desired-state.toml` persistence is in `main` and applies nothing automatically.

Before reconciliation:

- retire or harden legacy AppConfig/path APIs and remove current-directory fallbacks (#113);
- resolve/remove the historical sessiond development modes whose old Nix options were silently ineffective; current module fails closed instead (#122);
- define policy that compares authoritative Observed state before proposing action;
- preserve `Accepted != Applied` and explicit Pending semantics;
- add integration tests before connecting lifecycle events to execution;
- loading missing/default/corrupt configuration must never synthesize a hardware action.

## Milestone 3 — Preferences and desktop integration

**Status: PARTIAL / FAIL-CLOSED WHERE UNWIRED**

Completed:

- versioned `preferences.toml`;
- durable atomic writes and permission preservation;
- persisted Dark/Light theme;
- Start Minimized is read/applied at runtime;
- redacted config warnings;
- independent XDG window-state store;
- XDG autostart backend and Slint user-only toggle contract;
- fake main-window Run on Startup toggle removed;
- desktop/AppStream metadata and Nix installation wiring.

Next:

- finish current-main Rust lifecycle glue for Run on Startup (#110); until then Preferences toggle stays disabled;
- finish Start Minimized UI editing plus honest window-state/close behavior (#121); do not fake Wayland positioning or tray lifecycle;
- keep automation policy and desired hardware state separate from UI preferences;
- repeat packaged metadata validation on the final release revision.

## Milestone 4 — Fans and telemetry

**Status: FAN READS AVAILABLE; FAN WRITES BLOCKED; TELEMETRY EVIDENCE HARDENING OPEN**

Telemetry provider and worker-owned polling are present in `main`.

Telemetry next step:

- distinguish useful fresh telemetry from empty/partial successful provider calls while preserving independent metrics and field-local failures (#117).

Fan reads include active and profile-specific curves, but the current fan evidence model is incomplete. Fan mutation/reset implementation exists but is **not an accepted production write path**. FansWindow mutation controls and packaged fan polkit authorization are fail-closed.

Mandatory before any fan write is re-enabled:

1. preserve authoritative `CurveData.enabled` on custom writes and verify it in post-write read-back (#104);
2. make Factory Defaults restore the previous performance profile even when reset fails (#105);
3. remove CPU/GPU cross-inference and support asymmetric fan evidence where valid (#109);
4. carry stored profile curve `enabled` state through Session1/UI (#116);
5. make effective capability/write status reflect the current safety/polkit block (#120);
6. require `FanCurveHwState::Ready` in the application mutation guard (#104);
7. obtain executable green tests/CI on the exact revision;
8. perform controlled dated hardware validation and confirm final fan/profile state.

## Milestone 5 — Diagnostics and supportability

**Status: TESTED FOUNDATIONS; LIFECYCLE WIRING PARTIAL / FAIL-CLOSED**

Integrated:

- typed diagnostics domain;
- privacy-safe metadata sources;
- service/capability/GPU/telemetry/display observations;
- application collector;
- UI DTO;
- allowlisted versioned text/JSON exporters;
- end-to-end pure-data regression;
- read-only production DiagnosticsRuntime;
- pure DiagnosticsWindowModel;
- typed DiagnosticsWindow surface.

Next:

- finish current-main Diagnostics open/refresh lifecycle wiring (#111); until then Refresh stays disabled and the window reports unavailable;
- keep export collection strictly allowlisted and privacy-bounded.

## Milestone 6 — GPU product policy

**Status: BLOCKED**

Eco / Standard / Ultimate / Optimized are product-level policy, not aliases for one backend enum. Production raw GPU mutation remains disabled. Do not enable product controls until mapping, ownership, pending reboot/logout requirements and confirmation semantics are proven and tested.

## Milestone 7 — Power limits and extended ASUS controls

**Status: BLOCKED / UNKNOWN BY CONCEPT**

Typed scaffolding exists for selected concepts. Main-window Screen/Visual/Slash/Keyboard preview controls are disabled and explicitly labelled Preview until a production capability/backend is connected.

Follow concept-specific readiness evidence. Do not infer support from DMI model names, generic firmware attributes or another ASUS feature with a similar label.

## Milestone 8 — CLI and release packaging

**Status: PARTIAL**

Completed:

- desktop/AppStream metadata and Nix installation wiring;
- release evidence taxonomy;
- support-matrix schema and validation tooling;
- repository/homepage metadata normalized to the actual repository;
- unimplemented `orbisctl` fails explicitly with exit code 2 instead of silently reporting success.

Remaining:

- implement minimal read-only `orbisctl` first (#119);
- remove mock/test-support from the default release graph and replace fixture-derived production initial state/version after executable CI returns (#115);
- enforce the user-session security invariant by rejecting interactive GUI root execution (#125);
- obtain green executable CI on final `main` (#106);
- perform final package/metadata acceptance on the release revision.

## Release gate

A beta/release candidate requires:

- intended baseline in `main`;
- no known unsafe write path enabled by default;
- provider operations and capability/status refresh bounded against hangs (#123);
- relevant Cargo integration checks green;
- `nix flake check` executed and green;
- package/metadata acceptance complete;
- current issues/PR state accurately represents unfinished work;
- dated evidence for device-specific claims;
- unsupported or incomplete controls shown honestly as unavailable/unknown;
- application identity decision recorded before stable release (#124);
- interactive GUI user-session boundary enforced (#125);
- after CI recovery, `main` protected by required real checks (#114).