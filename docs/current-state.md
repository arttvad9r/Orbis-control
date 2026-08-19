# Current State

> Роль: **CURRENT STATUS**. Краткий operational baseline фактической production-линии.
> Обновлено: **2026-08-19**. `main` является текущей интеграционной базой после
> объединения production hardening.
>
> Для release-claims используйте [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md):
> `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Orbis уже имеет рабочие production vertical slices для Battery, Performance и независимых GPU primitives. Узкие privileged mutations проходят только через typed `Hardware1` → polkit → bounded backend; `sessiond` остаётся read/session boundary и не является mutation deputy.

`main` также содержит production telemetry polling, profile-specific fan reads, безопасное preferences persistence, XDG window state, generic desired-state persistence, Desired/Observed/Pending + lifecycle domain foundations, typed diagnostics snapshot/export/runtime/window-model layers, desktop/AppStream packaging integration и support-matrix tooling. Fan mutation implementation присутствует, но временно **заблокирована fail-closed** после подтверждённых write-contract рисков (#104/#105/#109/#116).

Главный текущий release blocker: GitHub Actions не выполняет repository checks. Ранние jobs падали до первого step (`steps=[]`), а свежие push-коммиты `main` вообще не получают workflow run despite configured push trigger (#106). Это не является доказанным `cargo`/`nix` regression, но release gate закрыт до реально выполненного green `nix flake check`.

## Major areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | IMPLEMENTED | Production hardening merged into `main`; open PR count is zero. Old non-mergeable work was normalized into current-main issues. |
| Remote branches | CLEANUP PENDING — #118 | 139 obsolete/validation `agent/*` refs remain because the available connector cannot delete refs. |
| Rust/build contract | IMPLEMENTED | Workspace/toolchain pinned to Rust **1.87**, matching the locked UI dependency graph. |
| Core/domain | IMPLEMENTED | Typed domain/invariants, explicit capability evidence states, Desired/Observed/Pending values and inert lifecycle events. |
| Config/preferences | TESTED | Versioned XDG `preferences.toml`, atomic/durable writes, permission preservation, Dark/Light persistence, Start Minimized, redacted warning diagnostics. |
| Desired-state storage | TESTED FOUNDATION | Generic versioned `desired-state.toml` storage is integrated and inert; it does not apply settings automatically. Legacy AppConfig/path helpers require hardening before reconciliation (#113). |
| Window state | TESTED | Independent versioned XDG state store for window position; not mixed with preferences. |
| XDG Run on Startup | PARTIAL / FAIL-CLOSED UI | Safe owned-entry backend, exports and Slint callback contract are integrated. Until #110 is completed, the Preferences toggle is disabled and the main-window fake local toggle is removed. |
| Capabilities | IMPLEMENTED WITH OPEN HARDENING | Runtime registry/probes exist, but write-owner evidence, explicit-refresh freshness and fan per-operation granularity still have correctness issues (#107/#109/#112). |
| Battery read | LIVE-VALIDATED | UPower/asusd/kernel semantics through Session1; no mock fallback in production. |
| Battery mutation | LIVE-VALIDATED HISTORICAL / EVIDENCE HARDENING OPEN | Controlled `100 → 80 → 100` evidence exists, but current mutation-status liveness and discovery classification need #107/#108. |
| Battery UI evidence | FAIL-CLOSED | Numeric threshold is shown only while `ChargeLimitState::Ready`; Loading/Unavailable no longer expose the mock fixture value as authoritative threshold. |
| Performance read | LIVE-VALIDATED | Kernel `platform_profile` via Session1 with authoritative fresh reads. |
| Performance mutation | LIVE-VALIDATED | Typed Hardware1/polkit/kernel path with read-back; controlled `Balanced → Silent → Balanced` evidence. Performance VM now explicitly runs with UPower disabled to verify Session1 capability isolation. |
| GPU primitives | LIVE-VALIDATED (read) | Runtime power, physical MUX and access policy are separate production read concepts. |
| GPU product mode | BLOCKED | Eco/Standard/Ultimate/Optimized production mapping/mutation is not proven; production raw GPU mutation remains disabled. |
| Telemetry | IMPLEMENTED / TESTED WITH EVIDENCE GAP — #117 | Production sysfs telemetry provider and polling exist, but empty/partial successful attempts are currently too easily labelled fresh/available. |
| Fan curve reads | IMPLEMENTED / TESTED WITH EVIDENCE GAPS | Profile-specific read is Session1 → sessiond → asusd; active curve remains sysfs. CPU/GPU support is over-aggregated (#109), and stored `enabled` state is lost before UI (#116). |
| Fan curve mutation/reset | **BLOCKED — #104/#105/#109/#116** | Custom writes can alter `CurveData.enabled`; upstream Factory Defaults has a failure-restoration risk; capability/evidence model is incomplete. Packaged default polkit authorization and FansWindow mutation UI are disabled. |
| Panel/Aura write evidence | HARDENING OPEN | Setter semantics are typed, but mutation status can claim Supported without proving the actual asusd owner/interface is reachable (#107). |
| `sessiond` resilience | TESTED | Battery/UPower discovery is lazy/capability-local; UPower absence no longer blocks independent Performance/GPU/Fan Session1 startup. |
| NixOS UPower integration | TESTED | Orbis enables UPower with `lib.mkDefault true`; explicit host override remains stronger; no hard service lifecycle coupling. |
| Privileged helper | IMPLEMENTED / historically LIVE-VALIDATED | Typed Hardware1 helper; no generic sysfs/filesystem/shell/D-Bus proxy. |
| Diagnostics core/export | TESTED | Typed diagnostics domain/providers/collector/DTO and privacy-bounded text/JSON exporters are integrated; end-to-end pure-data regression is included. |
| Diagnostics runtime/window model | TESTED FOUNDATION / FAIL-CLOSED UI | Read-only production source orchestration, presentation model and typed Slint surface are integrated. Until #111 is completed, the window reports unavailable and Refresh is disabled. |
| Desktop/AppStream packaging | TESTED | Canonical metadata sources and Nix package installation wiring are integrated; workspace/Nix repository metadata now points to the actual repository. |
| Support matrix tooling | TESTED | Schema, evidence rules, fixtures and locked-nixpkgs validator are integrated; runtime capability detection remains probe-driven. |
| Reconciliation engine | NOT IMPLEMENTED | Foundations are present, but no startup/resume planner/executor automatically applies desired state. |
| CLI | NOT IMPLEMENTED — #119 | `orbisctl` no longer silently succeeds; until implemented it prints an explicit message and exits 2. |
| Release dependency hygiene | OPEN — #115 | UI default feature graph still carries mock/test-support code; production runtime does not use it as authoritative fallback. |
| CI/release gate | BLOCKED — #106 | Actions is not executing current-main checks; green executable repository validation is required. |
| Main protection | DEFERRED — #114 | `main` is currently unprotected; enable required checks only after CI is executable. |

## Production boundaries

### Read path

```text
UPower / kernel / supergfxd / ASUS firmware attributes / asusd
→ orbis-sessiond → Session1
→ orbis-session-client / providers
→ application worker → GUI
```

Read failures are capability-local. `Unsupported`, `Unavailable`, `PermissionDenied` and `Unknown` are not interchangeable and must not be normalized into fake values.

### Mutation path

```text
GUI/application original caller
→ Hardware1 system bus
→ per-capability polkit authorization
→ typed bounded backend
→ authoritative read-back when the operation can be confirmed
```

The GUI does not run as root. `sessiond` does not proxy privileged mutations. Caller-provided paths, shell commands and generic privileged writers are outside the architecture.

`ApplyResult::Accepted` is explicitly **not** `Applied`; accepted/unconfirmed state must not become authoritative observed state without confirmation. See ADR 0012.

## Confirmed live evidence retained from earlier baseline

The following claims remain revision-scoped historical evidence and must be revalidated after relevant behavior changes:

- Battery read path and Battery mutation `100 → 80 → 100` with final state restored.
- Performance read path and controlled GUI mutation `Balanced → Silent → Balanced` with final state restored.
- GPU primitive reads for power/MUX/access matched their authoritative backends on the validated FA707NV system.
- Hardware1 service/sandbox and caller authorization were live-validated on the documented NixOS generation used by those tests.

These observations are not universal ASUS specifications and must not be converted into model tables or guessed support.

## Current blockers / unfinished work

1. **Fan write safety:** fix enabled-state preservation (#104), Factory Defaults profile restoration (#105), CPU/GPU granularity (#109) and enabled-state read evidence (#116) before re-enabling fan writes.
2. **Release CI:** restore executable GitHub Actions and obtain a green current-`main` `nix flake check` (#106).
3. **Write evidence:** prove actual Battery/Panel/Aura mutation owners are reachable (#107), preserve Battery discovery failures accurately (#108), and refresh write evidence on explicit capability refresh (#112).
4. Finish XDG Run on Startup lifecycle wiring against current `main.rs` (#110); backend exists and UI is disabled until connected.
5. Finish Diagnostics lifecycle/refresh wiring against current `main.rs` (#111); foundations exist and UI is disabled until connected.
6. Harden/remove legacy config/path APIs before reconciliation (#113), then design reconciliation semantics without auto-applying defaults.
7. Refine empty/partial telemetry freshness/evidence semantics (#117).
8. Keep GPU product mode, power limits and extended ASUS controls disabled/unknown until concept-specific evidence exists.
9. Implement minimal read-only `orbisctl` (#119); current binary fails explicitly rather than pretending success.
10. Remove mock/test-support from the default release UI graph after executable CI returns (#115).
11. Protect `main` with required real checks after CI recovery (#114).
12. Prune obsolete remote `agent/*` refs when branch-delete access is available (#118).
13. Obtain final packaged acceptance evidence on the exact release revision.

## Evidence and design references

- [`architecture.md`](architecture.md)
- [`verification.md`](verification.md)
- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`security-boundary-audit-2026-08-19.md`](security-boundary-audit-2026-08-19.md)
- [`threat-model.md`](threat-model.md)
- [`support-matrix-schema.md`](support-matrix-schema.md)
- [`multi-model-discovery-research.md`](multi-model-discovery-research.md)
- [`power-limit-readiness-audit.md`](power-limit-readiness-audit.md)
- [`extended-asus-controls-readiness.md`](extended-asus-controls-readiness.md)
- ADRs under [`adr/`](adr/)

## Rule for updating this file

Update this document whenever production behavior, capability evidence, deployment state or a release gate changes. Tests establish `TESTED`; they do not establish `LIVE-VALIDATED` hardware behavior.