# Roadmap

> Роль: **FUTURE PLAN**. Очередность работ без календарных обещаний.
> Фактическое состояние и evidence — в [`current-state.md`](current-state.md).
> Обновлено: **2026-08-20**.

## Principles

1. Сначала safety blockers и runtime reliability, затем новые функции.
2. Пока executable GitHub Actions недоступен (#106), разрешён offline-safe cleanup и narrow source work, но он не считается green CI или release evidence.
3. Support определяется typed probes/evidence, не названием модели.
4. Read/write evidence хранится раздельно.
5. `Accepted` не считается `Applied`.
6. Live claims revision-scoped.
7. Неполный/conflicting control остаётся fail-closed до исправления контракта.
8. Dated audits и старые remediation plans не являются backlog source of truth; активная работа должна быть отражена в Issues и этом roadmap.

## Milestone 0 — Repository/release baseline

**Status: CONSOLIDATED; EXECUTABLE CI EXTERNALLY BLOCKED**

Completed/in review:

- production hardening integrated into `main`;
- Rust MSRV/toolchain moved to 1.87;
- Battery/UPower startup resilience integrated;
- preferences/window/desired-state/lifecycle foundations integrated;
- diagnostics foundations integrated;
- desktop/AppStream/Nix packaging and support-matrix tooling integrated;
- historical broken sessiond `mockDevice` / `readOnlyEmpty` options removed (#122 resolved);
- obsolete self-publishing/targeted validation workflows removed in repository cleanup PR #127;
- private session transcript removed from repository cleanup PR #127;
- stray binary ADR archive removed; ADR directory returns to reviewable Markdown-only decisions;
- documentation hierarchy normalized with explicit canonical vs historical classification.

Remaining:

- #106: restore executable GitHub Actions when repository/account infrastructure permits it; until then do not manufacture substitute CI evidence;
- #114: after a real executable check exists, protect `main` with required checks;
- #118: prune obsolete `agent/*` refs when delete-ref access/workflow is available;
- #124: confirm permanent reverse-DNS identity before stable release;
- merge repository cleanup PR only after human review; lack of CI must remain visible in the merge decision.

## Milestone 1 — Runtime reliability and truthful capability state

**Status: HIGHEST-VALUE SOURCE WORK WHILE CI IS BLOCKED**

Priority order:

1. #125 — reject raw GUI execution as euid 0 before user config/runtime/bus setup;
2. #123 — enforce bounded provider/status execution in the sequential application path without unsafe mutation retry semantics;
3. #112 — make explicit capability refresh use the same mutation-status re-query path as periodic refresh;
4. #117 — distinguish useful fresh telemetry from empty/partial successful calls;
5. #107 — prove Battery mutation owner/interface liveness dynamically;
6. #120 — make effective write capability include product/policy safety blocks.

Source patches may be prepared without Actions if they remain narrow and reviewable,
but runtime-sensitive changes should stay unclaimed/unreleased until executable tests
and package validation return.

## Milestone 2 — Stable production capabilities

**Status: CORE READ PATHS STABLE; WRITE EVIDENCE HARDENING ACTIVE**

Historical revision-scoped validated areas:

- Battery read and controlled threshold mutation;
- Performance read and controlled profile mutation;
- independent GPU power / MUX / access reads;
- narrow typed Hardware1 privilege boundary.

Hardening required:

- #108 — execute validation for the already-integrated Battery discovery classification fix;
- #126 — validate standalone hardwared sandbox/capability parity on an executable system;
- retain GPU/Panel/Keyboard/Aura disabled until concept-specific mutation ownership and product policy are proven.

## Milestone 3 — Lifecycle and desired state

**Status: FOUNDATIONS INTEGRATED; RECONCILIATION NOT IMPLEMENTED**

Completed:

- Session1 startup capability-local;
- Desired / Observed / Pending foundations exist;
- inert lifecycle event values exist;
- versioned `desired-state.toml` exists and applies nothing automatically;
- misleading historical sessiond dev-mode options removed.

Before reconciliation:

- #113 — retire remaining legacy AppConfig/path compatibility symbols after executable compatibility validation;
- compare authoritative Observed state before proposing any action;
- preserve `Accepted != Applied` and explicit Pending semantics;
- add integration evidence before lifecycle events trigger execution;
- missing/default/corrupt config must never synthesize hardware actions.

## Milestone 4 — Preferences and desktop integration

**Status: PARTIAL / FAIL-CLOSED WHERE UNWIRED**

Next:

- #110 — finish XDG Run on Startup lifecycle wiring;
- #121 — finish window-state, close and tray semantics;
- keep automation policy and desired hardware state separate from UI preferences;
- retain redacted preferences warning logging as a privacy invariant;
- repeat packaged metadata validation on final release revision.

## Milestone 5 — Fans and telemetry

**Status: FAN READS AVAILABLE; FAN WRITES HARD-BLOCKED**

Telemetry work is #117 under Milestone 1 because it affects runtime truth globally.

Mandatory before fan writes are re-enabled:

1. #109 — stop CPU/GPU support cross-inference;
2. #116 — carry stored `enabled` state through Session1/UI;
3. #104 — preserve authoritative `CurveData.enabled` on custom writes and require Ready state in mutation guard;
4. #105 — make Factory Defaults restore the previous performance profile on every failure path;
5. #120 — expose effective safety/policy block honestly;
6. executable green tests/CI on exact revision;
7. controlled dated hardware validation with final fan/profile state proof.

Do not reorder this milestone by implementing write enablement before the read/evidence
model is correct.

## Milestone 6 — Diagnostics/supportability

**Status: TESTED FOUNDATIONS; LIFECYCLE WIRING FAIL-CLOSED**

Next:

- #111 — finish Diagnostics open/refresh lifecycle wiring;
- keep exports allowlisted/privacy-bounded;
- carry capability/telemetry evidence distinctions without fake defaults.

## Milestone 7 — Release graph and CLI

**Status: IMPLEMENTED FOUNDATIONS / VALIDATION OPEN**

Next:

- #115 — remove mock/test-support from the default release dependency graph and replace fixture-derived production initial state/version;
- #119 — executable workspace/integration validation and CLI polish;
- #124 — finalize application identity;
- final package/AppStream/desktop acceptance after executable validation returns.

## Milestone 8 — GPU product policy

**Status: BLOCKED**

Eco / Standard / Ultimate / Optimized are product policy, not aliases for one
backend enum. Production raw GPU mutation remains disabled until mapping,
ownership, restart/reboot requirements and confirmation semantics are proven.

## Milestone 9 — Extended ASUS controls

**Status: BLOCKED / UNKNOWN BY CONCEPT**

Power limits, display/lighting and other extended controls remain evidence-gated.
Preview UI stays disabled until a production capability/backend is connected. Do
not infer support from DMI model names or unrelated firmware attributes.

## Release gate

A release candidate requires:

- intended baseline integrated in `main`;
- no known unsafe write enabled by default;
- provider/status operations bounded against hangs (#123);
- GUI user-session boundary enforced (#125);
- relevant Cargo tests/clippy executed and green;
- `nix flake check` executed and green;
- package/metadata acceptance complete;
- dated evidence for device-specific claims;
- unsupported/incomplete controls shown honestly;
- application identity decision recorded (#124);
- `main` protected by required real checks after CI recovery (#114).

Until #106 is executable again, repository cleanup, documentation normalization and
narrow source remediation may continue, but the release gate remains **BLOCKED**.
