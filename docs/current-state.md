# Current State

> Роль: **CURRENT STATUS**. Краткий operational baseline фактической production-линии.
> Обновлено: **2026-08-19**. `main` является текущей интеграционной базой после
> объединения production hardening.
>
> Для release-claims используйте [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md):
> `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Orbis уже имеет рабочие production vertical slices для Battery, Performance и независимых GPU primitives. Узкие privileged mutations проходят только через typed `Hardware1` → polkit → bounded backend; `sessiond` остаётся read/session boundary и не является mutation deputy.

`main` также содержит production telemetry polling, profile-specific fan reads, fan-curve mutation wiring, безопасное preferences persistence, XDG window state, typed diagnostics snapshot/export stack, desktop/AppStream packaging integration и support-matrix tooling.

Главный текущий release blocker: GitHub Actions job завершается `failure` **до выполнения workflow steps** (`steps=[]`, log blob недоступен). Это не является доказанным `cargo`/`nix` regression, но release gate остаётся закрытым до реально выполненного green `nix flake check`.

## Major areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | IMPLEMENTED | Consolidated production hardening merged into `main` on 2026-08-19. |
| Rust/build contract | IMPLEMENTED | Workspace/toolchain pinned to Rust **1.87**, matching the locked UI dependency graph. |
| Core/domain | IMPLEMENTED | Typed domain/invariants and explicit capability evidence states. |
| Config/preferences | TESTED | Versioned XDG `preferences.toml`, atomic/durable writes, permission preservation, Dark/Light persistence, Start Minimized, redacted warning diagnostics. |
| Window state | TESTED | Independent versioned XDG state store for window position; not mixed with preferences. |
| Capabilities | IMPLEMENTED | Runtime registry/probes for current production concepts; read/write evidence remains independent. |
| Battery read | LIVE-VALIDATED | UPower/asusd/kernel semantics through Session1; no mock fallback in production. |
| Battery mutation | LIVE-VALIDATED | Typed Hardware1/asusd path with authoritative read-back; historical controlled `100 → 80 → 100` evidence. |
| Performance read | LIVE-VALIDATED | Kernel `platform_profile` via Session1 with authoritative fresh reads. |
| Performance mutation | LIVE-VALIDATED | Typed Hardware1/polkit/kernel path with read-back; controlled `Balanced → Silent → Balanced` evidence. |
| GPU primitives | LIVE-VALIDATED (read) | Runtime power, physical MUX and access policy are separate production read concepts. |
| GPU product mode | BLOCKED | Eco/Standard/Ultimate/Optimized production mapping/mutation is not proven; controls remain unsupported/disabled. |
| Telemetry | IMPLEMENTED / TESTED | Production sysfs telemetry provider and worker-owned polling exist; this is not automatically live hardware acceptance evidence. |
| Fan curves | IMPLEMENTED / TESTED | Profile-specific read is Session1 → sessiond → asusd; active curve remains sysfs; typed Hardware1 mutation path exists. Live mutation acceptance remains UNKNOWN until dated hardware validation. |
| `sessiond` resilience | TESTED | Battery/UPower discovery is lazy/capability-local; UPower absence no longer blocks independent Performance/GPU/Fan Session1 startup. |
| NixOS UPower integration | TESTED | Orbis enables UPower with `lib.mkDefault true`; explicit host override remains stronger; no hard service lifecycle coupling. |
| Privileged helper | IMPLEMENTED / historically LIVE-VALIDATED | Typed Hardware1 helper; no generic sysfs/filesystem/shell/D-Bus proxy. |
| Diagnostics core/export | TESTED | Typed diagnostics domain/providers/collector/DTO and privacy-bounded text/JSON exporters are integrated in `main`; end-to-end pure-data regression is included. |
| Diagnostics window wiring | PARTIAL | Backend/export stack is integrated; the separate UI wiring branch currently conflicts with the consolidated `main.rs`/UI baseline. |
| Desktop/AppStream packaging | TESTED | Canonical metadata sources and Nix package installation wiring are integrated; prior targeted packaging validation built the package and asserted installation. |
| Support matrix tooling | TESTED | Schema, evidence rules, fixtures and locked-nixpkgs validator are integrated; runtime capability detection remains probe-driven. |
| CLI | NOT IMPLEMENTED | `orbisctl` binary remains a stub. |
| Automation/reconciliation | PARTIAL | Foundations exist in a conflicting branch; production Desired/Observed/Pending + lifecycle/reconciliation stack is not fully integrated. |
| XDG Run on Startup | PARTIAL | Backend + UI stack is implemented/tested in a branch but conflicts with the consolidated config/UI baseline. |
| CI/release gate | BLOCKED | GitHub Actions currently fails before exposing/executing steps; green `nix flake check` is still required. |

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
→ authoritative read-back
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

1. Restore executable GitHub Actions runs and obtain a green `nix flake check` on current `main`.
2. Resolve and integrate the XDG autostart backend/UI conflict (#57) against the consolidated preferences stack.
3. Reconcile the Desired/Observed/Pending + desired-state + lifecycle foundation (#59) against current config, then add reconciliation semantics separately.
4. Resolve Diagnostics window wiring (#101) against current `main.rs`/UI; the typed diagnostics/export backend is already integrated.
5. Perform dated live validation before claiming production fan mutation support.
6. Keep GPU product mode, power limits and extended ASUS controls disabled/unknown until concept-specific evidence exists.
7. Implement a real CLI; current `orbisctl` is a stub.
8. Factory Defaults / large visual UI stack (#2/#3/#4) remains outside `main` until the root hardware-related slice receives the required compile/live validation.

## Evidence and design references

- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`security-boundary-audit-2026-08-19.md`](security-boundary-audit-2026-08-19.md)
- [`support-matrix-schema.md`](support-matrix-schema.md)
- [`multi-model-discovery-research.md`](multi-model-discovery-research.md)
- [`power-limit-readiness-audit.md`](power-limit-readiness-audit.md)
- [`extended-asus-controls-readiness.md`](extended-asus-controls-readiness.md)
- [`release-metadata-design.md`](release-metadata-design.md)
- ADRs under [`adr/`](adr/)

## Rule for updating this file

Update this document whenever production behavior, capability evidence, deployment state or a release gate changes. Do not copy Draft PR claims here until the relevant implementation is integrated. Tests establish `TESTED`; they do not establish `LIVE-VALIDATED` hardware behavior.
