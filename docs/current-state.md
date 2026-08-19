# Current State

> Роль: **CURRENT STATUS**. Краткий operational baseline фактической production-линии.
> Обновлено: **2026-08-19**. До завершения интеграции этот документ описывает
> `chatgpt/production-hardening-20260818`; `main` может отставать.
>
> Для release-claims используйте [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md):
> `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Orbis уже имеет рабочие production vertical slices для Battery, Performance и независимых GPU primitives. Узкие privileged mutations проходят только через typed `Hardware1` → polkit → bounded backend; `sessiond` остаётся read/session boundary и не является mutation deputy.

Текущая интеграционная линия также содержит production telemetry polling, profile-specific fan reads, fan-curve mutation wiring, безопасное preferences persistence и XDG window state. При этом fan mutation не повышается до `LIVE-VALIDATED` без отдельного dated hardware evidence.

Главный текущий integration blocker: GitHub Actions `nix flake check` на актуальном hardening HEAD остаётся красным; до зелёного CI hardening не должен считаться release baseline.

## Major areas

| Area | Status | Current fact |
|---|---|---|
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
| CLI | NOT IMPLEMENTED | `orbisctl` binary remains a stub. |
| Automation/reconciliation | PARTIAL | Foundations exist in branches, but the production Desired/Observed/Pending + lifecycle/reconciliation stack is not fully integrated. |
| XDG Run on Startup | PARTIAL | Backend + UI stack is implemented/tested in a branch but currently conflicts with the consolidated config/UI integration and is not yet in hardening. |
| Diagnostics/export | PARTIAL | Typed/read-only stack exists in Draft branches; not yet consolidated into the production baseline. |
| CI/release gate | BLOCKED | Current hardening `flake-check` is red; exact GitHub job log retrieval is currently unavailable from the connector. |

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

1. Make `nix flake check` green on the integrated hardening HEAD and capture the failing check reason in-repo.
2. Resolve and integrate the XDG autostart backend/UI conflict against the consolidated preferences stack.
3. Reconcile the Desired/Observed/Pending + desired-state + lifecycle foundation against the updated config baseline, then add reconciliation semantics separately.
4. Consolidate the read-only diagnostics stack and privacy-bounded exporters.
5. Perform dated live validation before claiming production fan mutation support.
6. Keep GPU product mode, power limits and extended ASUS controls disabled/unknown until concept-specific evidence exists.
7. Implement a real CLI; current `orbisctl` is a stub.

## Evidence and design references

- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`security-boundary-audit-2026-08-19.md`](security-boundary-audit-2026-08-19.md)
- [`multi-model-discovery-research.md`](multi-model-discovery-research.md)
- [`power-limit-readiness-audit.md`](power-limit-readiness-audit.md)
- [`extended-asus-controls-readiness.md`](extended-asus-controls-readiness.md)
- [`release-metadata-design.md`](release-metadata-design.md)
- ADRs under [`adr/`](adr/)

## Rule for updating this file

Update this document whenever production behavior, capability evidence, deployment state or a release gate changes. Do not copy Draft PR claims here until the relevant implementation is integrated. Tests establish `TESTED`; they do not establish `LIVE-VALIDATED` hardware behavior.
