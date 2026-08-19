# Current State

> Роль: **CURRENT STATUS**. Operational baseline текущей production-линии.
> Обновлено: **2026-08-19**. Интеграционная база — `main`.
>
> Release claims используют [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md):
> `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Orbis имеет production vertical slices для Battery, Performance и независимых GPU read primitives. Privileged mutations идут только через typed `Hardware1` → original-caller polkit → bounded backend; `sessiond` остаётся user-session/read boundary и не является privileged deputy.

`main` также содержит production telemetry polling, profile-specific fan reads, hardened user preferences/state stores, desired-state/lifecycle foundations, diagnostics foundations, desktop/AppStream/Nix packaging и support-matrix tooling.

**Fan writes остаются fail-closed.** Подтверждены contract risks вокруг сохранения `CurveData.enabled`, Factory Defaults restoration и fan evidence granularity (#104/#105/#109/#116/#120). FansWindow mutation controls отключены, packaged fan polkit action default-deny.

**Panel Overdrive и Aura Static RGB writes также fail-closed в packaged policy**, пока #107 не докажет реальный mutation-owner liveness и release evidence. Battery/Performance остаются разрешёнными typed production slices; raw GPU production backend остаётся disabled.

**Главный release blocker — #106:** GitHub Actions не доходит до первого workflow step. Fresh pushes могут не получать run, rerun старого job снова завершился failure с `steps=[]`. Это не доказанный Cargo/Nix failure; executable validation текущего `main` отсутствует.

## Current areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | IMPLEMENTED | Production hardening integrated in `main`; open PR count = 0. |
| Rust/build contract | IMPLEMENTED | Workspace/toolchain MSRV pinned to Rust 1.87. |
| Remote branches | CLEANUP PENDING — #118 | Obsolete `agent/*` refs remain; available connector has no delete-ref operation. |
| Core/domain | IMPLEMENTED | Typed state/invariants, capability evidence, Desired/Observed/Pending and lifecycle values. |
| Preferences/config | HARDENED / COMPAT CLEANUP OPEN — #113 | Production stores are fail-closed. Legacy defaults are hardware-inert, legacy writes are durable/atomic, and Orbis' own legacy loader now rejects missing/relative HOME/XDG instead of using CWD. Historical public CWD-fallback helpers remain deprecated pending removal after executable compatibility validation. |
| Run on Startup | PARTIAL / FAIL-CLOSED — #110 | Safe XDG backend exists; fake main-window toggle removed; Preferences control stays disabled until current-main lifecycle glue is finished. |
| Sessiond dev modes | **REMOVED — #122 resolved** | Historical `mockDevice`/`readOnlyEmpty` options were deleted from the NixOS module because sessiond never implemented their argv contract. Future dev modes require a new explicit design. |
| Capabilities | IMPLEMENTED WITH HARDENING OPEN | Runtime registry exists; owner liveness, fan/effective-policy truth and explicit-refresh freshness remain open (#107/#109/#112/#120). |
| Battery read | LIVE-VALIDATED historical | Session1/UPower/asusd/kernel path; startup is capability-local. |
| Battery mutation | LIVE-VALIDATED historical / HARDENING OPEN | Controlled threshold evidence exists; status liveness/discovery classification still need #107/#108. |
| Performance read/write | LIVE-VALIDATED historical | Typed Session1 read + Hardware1/polkit write with read-back. VM intentionally disables UPower to verify capability isolation. |
| GPU primitives | LIVE-VALIDATED historical (read) | Power, physical MUX and access policy are distinct read concepts. |
| GPU product mode | BLOCKED | Product Eco/Standard/Ultimate/Optimized mutation mapping is not proven; raw production GPU mutation remains disabled. |
| Telemetry | IMPLEMENTED / TESTED WITH EVIDENCE GAP — #117 | Empty/partial successful calls can still be labelled fresh/available too easily. |
| Fan reads | IMPLEMENTED / TESTED WITH EVIDENCE GAPS | CPU/GPU evidence is over-aggregated (#109) and stored `enabled` state is lost before UI (#116). |
| Fan writes/reset | **BLOCKED** | #104/#105/#109/#116/#120 must be resolved before re-enabling writes. |
| Panel/Aura mutation | **POLICY-BLOCKED / EVIDENCE OPEN — #107** | Typed setters exist, but actual owner/interface liveness is not proven by current mutation status. Packaged polkit now denies these writes by default until evidence is completed. |
| Provider execution | RELIABILITY GAP — #123 | `Provider::timeout()` exists but is not generically enforced in the sequential worker/application path. |
| GUI root boundary | NOT ENFORCED — #125 | Packaged desktop flow is user-level, but raw GUI binary does not yet reject euid 0. |
| Diagnostics | TESTED FOUNDATION / FAIL-CLOSED UI — #111 | Runtime/model/export layers exist; current window Refresh stays disabled until lifecycle glue is connected. |
| Window lifecycle | PARTIAL — #121 | Start Minimized is applied; position/close/tray semantics remain incomplete. |
| CLI | **IMPLEMENTED READ-ONLY / NEEDS EXECUTABLE VALIDATION — #119** | `orbisctl --help`, `--version` and `status` exist. `status` performs only Session1 Battery/Performance/GPU reads, prints explicit evidence/error states and performs no Hardware1 mutation. Obsolete placeholder library target was removed. |
| Release dependency hygiene | OPEN — #115 | Default release graph still carries mock/test-support surface and production initial UI state/version originates from fixture code. |
| Standalone hardwared | STRUCTURALLY ALIGNED / VALIDATION OPEN — #126 | Standalone systemd sandbox uses the same exact direct-sysfs write allowlist as the full module (Performance + keyboard brightness); deploy script verifies effective `ReadWritePaths`. Runtime validation is still required. |
| Application identity | RELEASE DECISION — #124 | `io.github.orbiscontrol.*` is already ABI/desktop identity; permanence/ownership must be decided before stable release. |
| Main protection | DEFERRED — #114 | `main` remains unprotected until real CI can be made required safely. |
| CI/release gate | **BLOCKED — #106** | No trustworthy executable current-main repository validation yet. |

## Production boundaries

### Reads

```text
UPower / kernel / supergfxd / ASUS firmware attributes / asusd
→ orbis-sessiond → Session1
→ session client/providers
→ application worker → GUI / read-only CLI
```

Read failures remain capability-local. `Unsupported`, `Unavailable`, `PermissionDenied` and `Unknown` are not interchangeable and must never become fake defaults.

### Mutations

```text
GUI original caller
→ Hardware1 system bus
→ per-capability polkit authorization
→ typed bounded backend
→ authoritative read-back when confirmable
```

`sessiond` does not proxy privileged mutations. Caller-provided paths, arbitrary shell commands and generic privileged D-Bus/filesystem forwarding are outside the architecture. `ApplyResult::Accepted` is not `Applied`.

`orbisctl` currently has **no mutation command**.

## Current blockers / unfinished work

1. **Fan safety:** #104, #105, #109, #116, #120.
2. **Executable CI/release validation:** #106; then protect `main` under #114.
3. **Write/capability evidence:** #107, #108, #112, plus standalone validation #126. Panel/Aura remain policy-blocked until their evidence is fixed.
4. **Runtime reliability/security:** #123 and #125.
5. **Current-main lifecycle wiring:** Autostart #110, Diagnostics #111, window lifecycle #121.
6. **Persistence cleanup:** remove remaining deprecated compatibility path symbols under #113; reconciliation itself remains intentionally unimplemented and requires separate design/tests.
7. **Telemetry evidence semantics:** #117.
8. **Release graph/product honesty:** #115, application identity #124.
9. **CLI validation/polish:** #119; status is implemented read-only but still needs executable workspace/integration validation.
10. **Repository cleanup:** #118.
11. Keep GPU product mutation, power limits and other extended ASUS controls disabled/unknown until concept-specific evidence exists.
12. Obtain final packaged acceptance evidence on the exact release revision.

## Historical live evidence retained

Revision-scoped earlier evidence exists for:

- Battery read and controlled `100 → 80 → 100` mutation with restoration;
- Performance read and controlled `Balanced → Silent → Balanced` mutation with restoration;
- GPU primitive reads (power/MUX/access) on the validated FA707NV system;
- Hardware1 service/sandbox/caller authorization on the documented NixOS generation.

These are not universal ASUS support claims and must be revalidated after relevant behavior changes.

## References

- [`architecture.md`](architecture.md)
- [`verification.md`](verification.md)
- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`threat-model.md`](threat-model.md)
- [`roadmap.md`](roadmap.md)
- ADRs under [`adr/`](adr/)

Update this file whenever production behavior, capability evidence, deployment state or a release gate changes. Source inspection can establish `IMPLEMENTED`; it does not establish `TESTED`, `PACKAGED` or `LIVE-VALIDATED` without the corresponding executed evidence.
