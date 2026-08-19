# Architecture — Orbis Control

> Роль: **CURRENT DESIGN**. Этот документ фиксирует архитектурный контракт
> текущего `main`. Фактическая готовность и blockers — в
> [`current-state.md`](current-state.md), порядок работ — в
> [`roadmap.md`](roadmap.md), причины отдельных решений — в [`adr/`](adr/).

## 1. Основные принципы

- GUI работает от обычного пользователя и не получает root.
- `sessiond` — read/session boundary; он не является privileged mutation deputy.
- Privileged mutations проходят только через узкий typed `Hardware1` service.
- Никаких generic sysfs/filesystem/shell/D-Bus proxy APIs.
- Capability support определяется runtime evidence/probes, а не DMI model name.
- Read и write evidence независимы.
- `Unsupported`, `BackendMissing`, `TemporarilyUnavailable`, `PermissionDenied`
  и `Unknown` — разные состояния.
- `ApplyResult::Accepted` не означает `Applied`.
- Authoritative observed state обновляется только из read/read-back evidence.
- Неподтверждённая функция остаётся fail-closed в UI/policy.
- Live hardware claims всегда revision-scoped.

## 2. Слои

```text
Slint UI
  ↓ typed callbacks
orbis-ui sequential worker
  ↓
orbis-application services/commands
  ↓
provider traits + capability registry
  ├─ Session1 reads → orbis-sessiond → UPower / kernel / supergfxd / asusd
  ├─ direct read-only providers → sysfs / Wayland where appropriate
  └─ Hardware1 mutations → orbis-hardwared → polkit → bounded owner/backend
```

### Crates

| Crate | Ответственность |
|---|---|
| `orbis-core` | Domain types, invariants, ApplyResult, diagnostics/state models |
| `orbis-config` | XDG preferences/state/autostart/desired-state persistence |
| `orbis-capabilities` | Runtime capability registry, immutable snapshots, diagnostics projection |
| `orbis-providers` | Typed provider traits and production/test implementations |
| `orbis-application` | Use cases, command/read-back composition, diagnostics collector |
| `orbis-session-protocol` | Getter-oriented Session1 wire contract |
| `orbis-session-client` | Session1 read clients + caller-preserving Hardware1 clients |
| `orbis-sessiond` | User read daemon and session D-Bus service |
| `orbis-hardwared` | Root system service for bounded privileged operations |
| `orbis-ui` | Slint presentation, worker, production composition |
| `orbis-cli` | CLI crate; current binary is incomplete |
| `orbis-test-support` | Test/demo fixtures only; release-graph cleanup tracked separately |

## 3. Read path

Canonical session path:

```text
authoritative backend
→ orbis-sessiond
→ io.github.orbiscontrol.Session1
→ orbis-session-client/provider
→ application worker
→ GUI
```

Session1 reads are fresh and do not become mutation proxies.

Current production read concepts include Battery Charge Limit, Performance,
independent GPU power/MUX/access, profile-specific fan curves, telemetry and
selected ASUS/display diagnostics.

Battery discovery is **lazy/capability-local**. Absence or temporary failure of
UPower must not prevent Session1 from serving independent Performance/GPU/Fan
reads.

## 4. Privileged mutation path

```text
original GUI/application caller
→ system bus io.github.orbiscontrol.Hardware
→ Hardware1 typed method
→ polkit(system-bus-name of original sender)
→ bounded backend
→ authoritative read-back when confirmation is possible
```

`orbis-hardwared` runs as root under a restrictive systemd sandbox and empty
Linux capability bounding set. D-Bus policy controls bus ownership/access;
operation authorization remains inside Hardware1 via per-capability polkit
actions.

The existence of a Hardware1 method or backend object does **not** itself prove
write support; mutation status must be backed by concept-specific evidence.

Confirmed mutation patterns include Performance fixed-path write + read-back and
Battery typed asusd write + configured/effective confirmation. Historical live
evidence is revision-scoped and recorded in `current-state.md`.

Blocked patterns:

- GPU product mode remains disabled until product policy is proven;
- fan mutation/reset remains fail-closed until #104, #105, #109 and #116 are
  resolved and validated;
- power limits and extended ASUS controls remain unavailable/unknown without
  concept-specific evidence.

## 5. Capability registry

`orbis-capabilities` owns immutable runtime snapshots. Each capability carries
separate operation evidence; consumers must gate writes from
`operations.write.status`, not from overall status alone.

Production composition performs read probes, queries typed mutation-status
evidence, builds a whole snapshot, swaps it atomically and publishes generation
changes. Periodic refresh re-queries mutation status before rebuilding. Explicit
refresh must follow the same rule; the remaining consistency fix is #112.

Failure in one capability must not collapse unrelated capabilities.

## 6. Battery contract

```rust
ChargeLimit {
    enabled: bool,
    configured_percent: Option<Percent>,
    effective_percent: Option<Percent>,
    bounds: Option<ChargeLimitBounds>,
}
```

Sources remain distinct:

- `enabled` — UPower policy state;
- configured threshold — asusd;
- effective threshold — kernel power-supply attribute;
- bounds — only when actually supplied/proven by a backend.

No value is clamped or invented at domain/wire boundaries. Session1 decoding
handles D-Bus data as untrusted and rejects noncanonical payloads. Hardware1
mutation confirms fresh configured/effective state. Discovery and write-owner
liveness classification still require #107/#108.

## 7. Performance contract

Read:

```text
Session1 → kernel platform_profile / choices
```

Write:

```text
original caller → Hardware1 → polkit → fixed platform_profile path → fresh read-back
```

Unknown wire values are rejected. UI selection is authoritative only while the
read state is Ready. The NixOS Performance mutation VM intentionally disables
UPower so it also proves Battery availability is not a Session1 startup
dependency.

## 8. GPU contract

Keep these concepts independent:

```text
physical MUX
!= dGPU access policy
!= runtime dGPU power
!= requested product mode
!= pending reboot/logout requirement
```

Production supports independent reads. Product buttons
Eco/Standard/Ultimate/Optimized remain unavailable until a proven policy maps
those concepts and defines confirmation semantics. Raw backend enum values are
not a product API.

## 9. Fan contract

There are two distinct read concepts:

- **active curve** from `asus_custom_fan_curve` sysfs;
- **stored profile-specific curve** from asusd `FanCurveData(profile)`.

They must not be conflated. ASUS fan profile identity is lossless
(`Balanced/Performance/Quiet/LowPower`). Quiet and LowPower must not collapse in
mutation/wire state.

Current write path is deliberately blocked. Known contract issues:

- preserve custom `CurveData.enabled` (#104);
- restore original performance profile even when Factory Defaults fails (#105);
- do not infer CPU/GPU fan support from each other (#109);
- preserve profile-specific `enabled` evidence through Session1/UI (#116).

Until these are resolved, fan reads may be used but writes/default reset remain
fail-closed.

## 10. Telemetry

`SysfsTelemetryProvider` is read-only and dynamically discovers hwmon and
power-supply sources. It does not invent total power, fan percent or GPU
capability state. Independent metrics may be partial.

Collection freshness and useful-data coverage are separate concepts. Current
partial/empty-snapshot evidence semantics need refinement (#117); a successful
provider call must not automatically prove useful telemetry was observed.

## 11. Desired / Observed / Pending

```text
Desired = persisted/user/policy intent
Observed = authoritative runtime observation
Pending = explicit unconfirmed transition + requirement
```

`main` contains typed Desired/Observed/Pending values, lifecycle event values and
generic versioned desired-state persistence. There is **no production
reconciliation executor yet**.

Loading config/defaults must never itself trigger hardware mutation. Before
reconciliation is implemented, legacy config/path APIs must be retired or
hardened (#113).

## 12. Preferences and XDG state

Production persistence concerns remain separate:

- `preferences.toml` — application presentation/behavior;
- independent window state;
- owned XDG autostart desktop entry;
- `desired-state.toml` — typed desired-state foundation.

New stores use versioning, same-directory atomic replacement, durability and
permission handling. Legacy AppConfig/path helpers are compatibility-only (#113).

Run on Startup backend exists, but current Rust lifecycle glue is incomplete;
its UI remains disabled until #110 is completed.

## 13. Diagnostics

Diagnostics is read-only and privacy-bounded:

```text
approved typed sources
→ immutable DiagnosticsSnapshot
→ DiagnosticsUiDto
→ allowlisted presentation / text / JSON projection
```

It does not activate services, execute shell commands, dump arbitrary files or
environment, or collect serial/UUID/asset-tag identifiers.

Runtime/model/Slint foundations are integrated. The window stays fail-closed
until current-main lifecycle/refresh wiring is completed (#111).

## 14. Mock/test boundary

Mock providers and deterministic device fixtures exist for tests, screenshots
and development. They are not authoritative production hardware fallbacks.
The current release dependency graph still carries some mock/test-support code;
removal from the default production graph is tracked in #115.

## 15. Packaging and release gate

Nix packaging installs GUI/sessiond/CLI, Hardware1 support where applicable,
D-Bus policy, per-capability polkit policy, desktop/AppStream metadata and NixOS
services. Current fan polkit default is deny for active users until fan safety
issues are resolved.

A release candidate requires:

- no known unsafe write path enabled by default;
- executable Cargo/Nix checks on the exact revision;
- green `nix flake check`;
- package/metadata acceptance;
- dated evidence for device-specific claims;
- unsupported/incomplete controls shown honestly as unavailable/unknown.

GitHub Actions currently fails before its first workflow step (#106). After CI
is executable, `main` should be protected with required checks (#114).

## 16. Source-of-truth hierarchy

Use documents by role:

1. [`current-state.md`](current-state.md) — what is true now;
2. this file — architecture that must hold;
3. ADRs — why stable decisions were made;
4. [`roadmap.md`](roadmap.md) — future ordering;
5. research/audit documents — supporting evidence, not automatic production
   claims.

When code and a current-design document diverge, update the document or block
the behavior; do not silently reinterpret evidence.