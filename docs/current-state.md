# Current State

> Роль: **CURRENT STATUS**. Operational baseline текущей production-линии.
> Обновлено: **2026-08-20**. Интеграционная база — `main`.
>
> Release claims используют [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md):
> `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.
>
> Dated audits и старые remediation plans классифицированы в
> [`history.md`](history.md) и не переопределяют этот документ.

## Executive summary

Orbis имеет production vertical slices для Battery, Performance и независимых GPU
read primitives. Privileged mutations идут только через typed `Hardware1` →
original-caller polkit → bounded backend; `sessiond` остаётся user-session/read
boundary и не является privileged deputy.

**Production mutation composition намеренно минимальна:** live mutation backend
активен только для Performance и условно Battery. GPU/Fan/Panel/Keyboard/Aura
остаются в typed ABI/backend code, но production `hardwared` композирует для них
disabled backends, а packaged polkit default-deny. Root service имеет единственный
direct-sysfs writable path — `/sys/firmware/acpi/platform_profile`.

Fan reads через Session1 доступны; известные fan write/reset defects #104/#105
недостижимы через текущую production composition и должны быть исправлены до
любого будущего re-enable.

**Главный release blocker — #106:** GitHub Actions не предоставляет trustworthy
executable workflow evidence. Это infrastructure/repository Actions blocker, а
не доказанный Cargo/Nix failure. До восстановления Actions source inspection и
static repository review не должны называться green CI.

## Repository status

- `main` — интеграционная база; required checks пока не включены из-за #106/#114.
- Канонический general-purpose workflow — `.github/workflows/ci.yml`. Старые
  одноразовые targeted/self-publishing validation workflows удалены.
- Private session transcript и stray binary ADR archive удалены из рабочего
  документационного дерева.
- Canonical documentation hierarchy определена в `docs/README.md`; dated audits
  и superseded engineering records отделены через `docs/history.md`.
- Obsolete remote `agent/*` branches остаются отдельной cleanup-задачей #118.

## Current areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | CLEANED / IMPLEMENTED | Production hardening интегрирован; obsolete validation workflows и private/generated documentation artifacts удалены. |
| Rust/build contract | IMPLEMENTED | Workspace/toolchain MSRV закреплён на Rust 1.87. Старые audit claims про Rust 1.85 исторические. |
| Remote branches | CLEANUP PENDING — #118 | Obsolete `agent/*` refs ещё существуют. |
| Documentation | NORMALIZED | Canonical hierarchy определена; historical snapshots отделены через `history.md`; session transcript и ADR archive удалены. |
| Core/domain | IMPLEMENTED | Typed state/invariants, capability evidence, Desired/Observed/Pending и lifecycle values. |
| Preferences/config | HARDENED / COMPAT CLEANUP OPEN — #113 | Legacy defaults hardware-inert, writes durable/atomic, checked XDG resolvers fail closed; deprecated compatibility symbols ещё требуют removal после executable compatibility validation. |
| Preferences logging/privacy | IMPLEMENTED | `PreferencesWarningKind` production `Debug` выводит фиксированный `log_category()` и не раскрывает embedded parser/schema payload. Historical privacy audit описывает pre-fix state. |
| Run on Startup | PARTIAL / FAIL-CLOSED — #110 | Safe XDG backend существует; UI control остаётся disabled до lifecycle wiring. |
| Capabilities | IMPLEMENTED WITH HARDENING OPEN | Disabled mutation backends публикуют non-writable status; #120 consumer truth, #112 explicit-refresh freshness и #107 Battery owner liveness остаются. |
| Battery read | LIVE-VALIDATED historical | Session1/UPower/asusd/kernel path; startup capability-local. |
| Battery mutation | LIVE-VALIDATED historical / HARDENING OPEN | Controlled threshold evidence существует; #108 source fix интегрирован, dynamic asusd owner liveness остаётся #107. |
| Performance read/write | LIVE-VALIDATED historical | Typed Session1 read + Hardware1/polkit write with read-back. |
| GPU primitives | LIVE-VALIDATED historical (read) | Power, physical MUX и access policy — разные read concepts. |
| GPU product/raw mutation | BLOCKED | Product mapping не доказан; production raw GPU backend disabled, packaged action default-deny. |
| Telemetry | IMPLEMENTED / EVIDENCE GAP — #117 | Empty/partial successful snapshots могут выглядеть слишком fresh/available. |
| Fan reads | IMPLEMENTED / EVIDENCE GAPS | CPU/GPU evidence over-aggregated (#109); stored `enabled` теряется до UI (#116). |
| Fan writes/reset | HARD-BLOCKED | UI disabled + polkit default-deny + production disabled backend. #104/#105 mandatory before re-enable. |
| Panel mutation | HARD-BLOCKED | Hardware1 disabled + packaged default-deny. |
| Keyboard mutation | HARD-BLOCKED | Hardware1 disabled + packaged default-deny; keyboard sysfs не writable в root sandbox. |
| Aura mutation | HARD-BLOCKED | Hardware1 disabled + packaged default-deny. |
| Provider execution | RELIABILITY GAP — #123 | `Provider::timeout()` не generically enforced в sequential GUI/application paths; read-only `orbisctl status` применяет local timeouts. |
| GUI root boundary | NOT ENFORCED — #125 | Raw GUI binary ещё не reject euid 0. |
| Diagnostics | TESTED FOUNDATION / FAIL-CLOSED UI — #111 | Backend/model/export foundations существуют; Refresh lifecycle ещё не подключён. |
| Window lifecycle | PARTIAL — #121 | Start Minimized применяется; position/close/tray semantics incomplete. |
| CLI | IMPLEMENTED READ-ONLY / VALIDATION OPEN — #119 | `orbisctl status` выполняет bounded Session1 reads и не имеет mutation commands. |
| Release dependency hygiene | OPEN — #115 | Default UI release graph всё ещё несёт mock/test-support surface и fixture-derived initial state/version. |
| Hardwared sandbox | STRUCTURALLY MINIMIZED / VALIDATION OPEN — #126 | Direct sysfs writable surface ограничена `platform_profile`; executable VM/package evidence заблокирован #106. |
| Application identity | RELEASE DECISION — #124 | `io.github.orbiscontrol.*` используется как ABI/desktop identity; permanence/ownership нужно решить до stable release. |
| Main protection | DEFERRED — #114 | Required checks нельзя безопасно включать до восстановления executable CI. |
| CI/release gate | BLOCKED — #106 | Нет trustworthy executable current-main repository validation. |

## Production boundaries

### Reads

```text
UPower / kernel / supergfxd / ASUS firmware attributes / asusd
→ orbis-sessiond → Session1
→ session client/providers
→ application worker → GUI / read-only CLI
```

Read failures capability-local. `Unsupported`, `Unavailable`, `PermissionDenied`
и `Unknown` не взаимозаменяемы и не превращаются в fake defaults.

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

For Fan/Panel/Keyboard/Aura product block также представлен disabled Hardware1
backends. Raw GPU requests terminate in disabled backend. `sessiond` не proxy
privileged mutations, `orbisctl` не имеет mutation command.

## Work that can proceed without executable CI

Следующие категории допустимы как reviewable source work при условии, что они не
выдаются за executed validation:

1. documentation/source-of-truth cleanup;
2. deletion of obsolete repository-only workflows/artifacts;
3. static security/privacy inspection;
4. narrowly-scoped source fixes whose semantics can be reviewed without live hardware,
   но merge/release claim должен ждать executable validation, если изменение затрагивает runtime;
5. issue/roadmap normalization.

Privileged mutation semantics, fan re-enable, broad provider/runtime refactors и
release acceptance не должны продвигаться на основании одного static review.

## Current blockers / unfinished work

1. **Executable CI/release validation:** #106; затем required checks/main protection #114.
2. **Runtime reliability/security:** provider bounds #123 и GUI root boundary #125.
3. **Capability truth/freshness:** #107, #112, #120; #108 source fix ждёт execution evidence.
4. **Fan future re-enable:** #104, #105, #109, #116; текущий product path остаётся hard-blocked.
5. **Telemetry semantics:** #117.
6. **Release graph honesty:** #115.
7. **Lifecycle wiring:** #110, #111, #121.
8. **Legacy config/path cleanup:** #113.
9. **CLI executable validation/polish:** #119.
10. **Standalone/package validation:** #126.
11. **Application identity:** #124.
12. **Remote branch cleanup:** #118.
13. Product GPU, power limits и extended ASUS writes остаются disabled до concept-specific evidence и deliberate enablement.
14. Final packaged acceptance должен выполняться на exact release revision.

## Historical live evidence retained

Revision-scoped evidence существует для:

- Battery read и controlled `100 → 80 → 100` mutation with restoration;
- Performance read и controlled `Balanced → Silent → Balanced` mutation with restoration;
- GPU primitive reads на validated FA707NV system;
- Hardware1 service/sandbox/original-caller authorization на documented NixOS generation.

Это не universal ASUS support claims и не evidence для изменённой ревизии.

## References

- [`README.md`](README.md) — documentation hierarchy
- [`history.md`](history.md) — historical/superseded material index
- [`architecture.md`](architecture.md)
- [`verification.md`](verification.md)
- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`threat-model.md`](threat-model.md)
- [`roadmap.md`](roadmap.md)
- ADR under [`adr/`](adr/)

Обновляйте этот файл при изменении production behavior, capability evidence,
deployment state или release gate. Source inspection устанавливает максимум
`IMPLEMENTED`; executed evidence нужен для более сильных claims.
