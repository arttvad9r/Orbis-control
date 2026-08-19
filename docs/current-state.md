# Current State

> Роль: **CURRENT STATUS**. Operational baseline текущей production-линии.
> Обновлено: **2026-08-19**. Интеграционная база — `main`.
>
> Release claims используют [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md):
> `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Orbis имеет production vertical slices для Battery, Performance и независимых GPU read primitives. Privileged mutations идут только через typed `Hardware1` → original-caller polkit → bounded backend; `sessiond` остаётся user-session/read boundary и не является privileged deputy.

**Текущая production mutation composition намеренно минимальна:** live mutation backend активен только для Performance и условно Battery. GPU/Fan/Panel/Keyboard/Aura остаются в typed ABI/backend code, но production `hardwared` композирует для них disabled backends, которые возвращают `Unsupported`. Packaged polkit также default-deny для этих пяти capability, а root service имеет единственный direct-sysfs writable path — `/sys/firmware/acpi/platform_profile`.

Fan reads через Session1 остаются доступными; известные fan write/reset defects #104/#105 физически недостижимы через production Hardware1 даже при локальном ослаблении polkit.

**Главный release blocker — #106:** GitHub Actions не доходит до первого workflow step. Fresh pushes могут не получать run, rerun старого job снова завершился failure с `steps=[]`. Это не доказанный Cargo/Nix failure; executable validation текущего `main` отсутствует.

## Current areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | IMPLEMENTED | Production hardening integrated in `main`; open PR count = 0. |
| Rust/build contract | IMPLEMENTED | Workspace/toolchain MSRV pinned to Rust 1.87. |
| Remote branches | CLEANUP PENDING — #118 | Obsolete `agent/*` refs remain; available connector has no delete-ref operation. |
| Core/domain | IMPLEMENTED | Typed state/invariants, capability evidence, Desired/Observed/Pending and lifecycle values. |
| Preferences/config | HARDENED / COMPAT CLEANUP OPEN — #113 | Legacy defaults are hardware-inert, writes are durable/atomic, checked XDG resolvers reject missing/relative HOME/XDG, and Orbis' own compatibility loader no longer falls back to CWD. Historical public fallback helpers remain deprecated pending removal after executable compatibility validation. |
| Run on Startup | PARTIAL / FAIL-CLOSED — #110 | Safe XDG backend exists; fake main-window toggle removed; Preferences control stays disabled until current-main lifecycle glue is finished. |
| Sessiond dev modes | REMOVED — #122 resolved | Historical `mockDevice`/`readOnlyEmpty` options were deleted because sessiond never implemented their argv contract. |
| Capabilities | IMPLEMENTED WITH HARDENING OPEN | Disabled production mutation backends now publish non-writable status for Fan/Panel/Keyboard/Aura; consumer/diagnostics truth #120, explicit-refresh freshness #112 and Battery owner-liveness #107 remain. |
| Battery read | LIVE-VALIDATED historical | Session1/UPower/asusd/kernel path; startup is capability-local. |
| Battery mutation | LIVE-VALIDATED historical / HARDENING OPEN | Controlled threshold evidence exists. Discovery failure classification was fixed at source level under #108; dynamic asusd owner liveness still needs #107. |
| Performance read/write | LIVE-VALIDATED historical | Typed Session1 read + Hardware1/polkit write with read-back. VM intentionally disables UPower to verify capability isolation. |
| GPU primitives | LIVE-VALIDATED historical (read) | Power, physical MUX and access policy are distinct read concepts. |
| GPU product/raw mutation | BLOCKED | Product mapping is not proven. Production raw GPU backend returns `Unsupported`, and packaged GPU action is default-deny. |
| Telemetry | IMPLEMENTED / TESTED WITH EVIDENCE GAP — #117 | Empty/partial successful calls can still be labelled fresh/available too easily. |
| Fan reads | IMPLEMENTED / TESTED WITH EVIDENCE GAPS | CPU/GPU evidence is over-aggregated (#109) and stored `enabled` state is lost before UI (#116). |
| Fan writes/reset | HARD-BLOCKED | UI disabled + polkit default-deny + production `DisabledFanMutationBackend`. Dormant implementation still requires #104/#105 before any future re-enable. |
| Panel mutation | HARD-BLOCKED | Production Hardware1 reports `Unsupported`; packaged action default-deny. Dormant backend requires future owner/evidence work before deliberate re-enable. |
| Keyboard mutation | HARD-BLOCKED | Production Hardware1 reports `Unsupported`; packaged action default-deny; keyboard sysfs path is not writable in the root service sandbox. |
| Aura mutation | HARD-BLOCKED | Production Hardware1 reports `Unsupported`; packaged action default-deny. |
| Provider execution | RELIABILITY GAP — #123 | `Provider::timeout()` exists but is not generically enforced in sequential GUI worker/application paths. New `orbisctl status` does enforce provider timeouts locally. |
| GUI root boundary | NOT ENFORCED — #125 | Packaged desktop flow is user-level, but raw GUI binary does not yet reject euid 0. |
| Diagnostics | TESTED FOUNDATION / FAIL-CLOSED UI — #111 | Runtime/model/export layers exist; current window Refresh stays disabled until lifecycle glue is connected. |
| Window lifecycle | PARTIAL — #121 | Start Minimized is applied; position/close/tray semantics remain incomplete. |
| CLI | IMPLEMENTED READ-ONLY / NEEDS EXECUTABLE VALIDATION — #119 | `orbisctl --help`, `--version` and bounded `status` exist. `status` performs only Session1 Battery/Performance/GPU reads, applies provider timeouts, prints explicit evidence/error states, and performs no Hardware1 mutation. |
| Release dependency hygiene | OPEN — #115 | Default release graph still carries mock/test-support surface and production initial UI state/version originates from fixture code. |
| Full/standalone hardwared sandbox | STRUCTURALLY MINIMIZED / VALIDATION OPEN — #126 | Both deployment paths expose only `platform_profile` as direct sysfs writable. Standalone deploy verifies effective `ReadWritePaths` and fails if keyboard write appears. VM test asserts the same policy/sandbox matrix but cannot currently execute because #106 is blocked. |
| Application identity | RELEASE DECISION — #124 | `io.github.orbiscontrol.*` is already ABI/desktop identity; permanence/ownership must be decided before stable release. |
| Main protection | DEFERRED — #114 | `main` remains unprotected until real CI can be made required safely. |
| CI/release gate | BLOCKED — #106 | No trustworthy executable current-main repository validation yet. |

## Production boundaries

### Reads

```text
UPower / kernel / supergfxd / ASUS firmware attributes / asusd
→ orbis-sessiond → Session1
→ session client/providers
→ application worker → GUI / read-only CLI
```

Read failures remain capability-local. `Unsupported`, `Unavailable`, `PermissionDenied` and `Unknown` are not interchangeable and must never become fake defaults.

### Enabled mutations

```text
GUI original caller
→ Hardware1 system bus
→ per-capability polkit
→ Performance or Battery typed backend
→ authoritative read-back
```

Current packaged active-user defaults:

- Performance: allowed;
- Battery: allowed;
- GPU/Fan/Panel/Keyboard/Aura: denied.

For Fan/Panel/Keyboard/Aura the product block is also represented by explicit disabled Hardware1 backends. Raw GPU requests similarly terminate in the disabled backend. `sessiond` never proxies privileged mutations, and `orbisctl` has no mutation command.

## Current blockers / unfinished work

1. **Executable CI/release validation:** #106; then protect `main` under #114.
2. **Fan future re-enable prerequisites:** #104, #105, #109, #116. Current product path is hard-blocked.
3. **Capability truth/freshness:** #107 (Battery owner liveness), #112 (explicit refresh), #120 (consumer/diagnostics alignment). #108 source fix awaits execution.
4. **Runtime reliability/security:** #123 and #125.
5. **Current-main lifecycle wiring:** Autostart #110, Diagnostics #111, window lifecycle #121.
6. **Persistence cleanup:** remove remaining deprecated compatibility path symbols under #113; reconciliation remains intentionally unimplemented.
7. **Telemetry evidence semantics:** #117.
8. **Release graph/product honesty:** #115, application identity #124.
9. **CLI validation/polish:** #119.
10. **Standalone/package validation:** #126.
11. **Repository cleanup:** #118.
12. Keep product GPU, power limits and extended ASUS writes disabled until concept-specific evidence and deliberate product enablement exist.
13. Obtain final packaged acceptance evidence on the exact release revision.

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
