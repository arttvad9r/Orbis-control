# Roadmap

> Роль: **FUTURE PLAN**. Очередность работ без календарных обещаний.
> Фактическое состояние и evidence — в [`current-state.md`](current-state.md).
> Обновлено: **2026-08-19**.

## Principles

1. Сначала safety blockers, runtime reliability и executable green CI, затем новые функции.
2. Support определяется typed probes/evidence, не названием модели.
3. Read/write evidence хранится раздельно.
4. `Accepted` не считается `Applied`.
5. Live claims revision-scoped.
6. Неполный/conflicting control остаётся fail-closed до исправления контракта.

## Milestone 0 — Repository/release baseline

**Status: CONSOLIDATED; RELEASE CI BLOCKED**

Completed:

- production hardening integrated into `main`;
- Rust MSRV/toolchain moved to 1.87;
- Battery/UPower startup resilience integrated;
- preferences/window/desired-state/lifecycle foundations integrated;
- diagnostics foundations integrated;
- desktop/AppStream/Nix packaging and support-matrix tooling integrated;
- stale PR stack normalized; open PR count is zero;
- top-level product/architecture/security/status docs normalized;
- historical broken sessiond `mockDevice` / `readOnlyEmpty` Nix options removed (#122 resolved).

Remaining:

- restore executable GitHub Actions and obtain green current-main checks (#106);
- after CI recovery, protect `main` with required real checks (#114);
- prune obsolete `agent/*` refs when delete-ref access exists (#118);
- confirm permanent reverse-DNS identity before stable release (#124).

## Milestone 1 — Stable production capabilities

**Status: CORE READ PATHS STABLE; WRITE EVIDENCE HARDENING ACTIVE**

Historical revision-scoped validated areas:

- Battery read and controlled threshold mutation;
- Performance read and controlled profile mutation;
- independent GPU power / MUX / access reads;
- narrow typed Hardware1 privilege boundary.

Hardening required:

- prove Battery/Panel/Aura real mutation owners/interfaces before reporting write Supported (#107);
- preserve Battery discovery permission/transient failures (#108);
- explicit capability refresh must re-query mutation status (#112);
- effective write evidence must include product/policy safety blocks (#120);
- enforce bounded provider/status execution (#123);
- validate standalone hardwared sandbox/capability parity on an executable system (#126).

## Milestone 2 — Lifecycle and desired state

**Status: FOUNDATIONS INTEGRATED; RECONCILIATION NOT IMPLEMENTED**

Completed:

- Session1 startup is capability-local;
- Desired / Observed / Pending domain foundations exist;
- inert lifecycle event values exist;
- versioned `desired-state.toml` exists and applies nothing automatically;
- misleading historical sessiond dev-mode options were removed (#122).

Before reconciliation:

- retire/harden legacy AppConfig/path APIs and CWD fallbacks (#113);
- compare authoritative Observed state before proposing any action;
- preserve `Accepted != Applied` and explicit Pending semantics;
- add integration tests before lifecycle events trigger execution;
- missing/default/corrupt config must never synthesize hardware actions.

## Milestone 3 — Preferences and desktop integration

**Status: PARTIAL / FAIL-CLOSED WHERE UNWIRED**

Next:

- finish XDG Run on Startup current-main lifecycle wiring (#110);
- finish Start Minimized UI editing plus honest window-state/close behavior (#121);
- keep automation policy and desired hardware state separate from UI preferences;
- repeat packaged metadata validation on final release revision.

## Milestone 4 — Fans and telemetry

**Status: FAN READS AVAILABLE; FAN WRITES BLOCKED**

Telemetry:

- distinguish useful fresh telemetry from empty/partial successful calls while preserving field-local evidence (#117).

Mandatory before fan writes are re-enabled:

1. preserve authoritative `CurveData.enabled` on custom writes and read-back (#104);
2. guarantee Factory Defaults restores previous performance profile on failure (#105);
3. remove CPU/GPU support cross-inference (#109);
4. carry stored `enabled` state through Session1/UI (#116);
5. represent current safety/polkit block in effective write evidence (#120);
6. require `FanCurveHwState::Ready` in the application mutation guard (#104);
7. obtain executable green tests/CI on the exact revision;
8. perform controlled dated hardware validation and confirm final fan/profile state.

## Milestone 5 — Diagnostics/supportability

**Status: TESTED FOUNDATIONS; LIFECYCLE WIRING FAIL-CLOSED**

Next:

- finish Diagnostics open/refresh lifecycle wiring (#111);
- keep exports allowlisted/privacy-bounded;
- carry capability/telemetry evidence distinctions without fake defaults.

## Milestone 6 — GPU product policy

**Status: BLOCKED**

Eco / Standard / Ultimate / Optimized are product policy, not aliases for one backend enum. Production raw GPU mutation remains disabled until mapping, ownership, pending restart/reboot requirements and confirmation semantics are proven.

## Milestone 7 — Extended ASUS controls

**Status: BLOCKED / UNKNOWN BY CONCEPT**

Power limits, display/lighting and other extended controls remain evidence-gated. Preview UI stays disabled until a production capability/backend is connected. Do not infer support from DMI model names or unrelated firmware attributes.

## Milestone 8 — CLI and release packaging

**Status: PARTIAL IMPLEMENTATION**

Completed:

- desktop/AppStream metadata and Nix installation wiring;
- support-matrix schema/evidence tooling;
- repository/homepage metadata normalization;
- `orbisctl --help` / `--version`;
- **read-only `orbisctl status`** using existing Session1 providers for Battery, Performance and GPU power/MUX/access; no Hardware1 mutation path.

Remaining:

- executable workspace/integration validation and final CLI polish (#119);
- remove mock/test-support from default release graph and replace fixture-derived production initial state/version (#115);
- enforce raw GUI euid-0 rejection (#125);
- obtain green executable CI (#106);
- perform final package/metadata acceptance.

## Release gate

A release candidate requires:

- intended baseline in `main`;
- no known unsafe write enabled by default;
- provider/status operations bounded against hangs (#123);
- relevant Cargo tests/clippy executed and green;
- `nix flake check` executed and green;
- package/metadata acceptance complete;
- dated evidence for device-specific claims;
- unsupported/incomplete controls shown honestly;
- application identity decision recorded (#124);
- GUI user-session boundary enforced (#125);
- `main` protected by required real checks after CI recovery (#114).
