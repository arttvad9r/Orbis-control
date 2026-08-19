# Architecture — Orbis Control

> Роль: **CURRENT DESIGN**. Этот документ фиксирует архитектурный контракт
> текущего `main`. Фактическая готовность и blockers — в
> [`current-state.md`](current-state.md), порядок работ — в
> [`roadmap.md`](roadmap.md), причины отдельных решений — в [`adr/`](adr/).

## 1. Основные принципы

- GUI работает от обычного пользователя; raw euid-0 guard ещё должен быть enforced (#125).
- `sessiond` — read/session boundary; он не является privileged mutation deputy.
- Privileged mutations проходят только через узкий typed `Hardware1` service.
- Никаких generic sysfs/filesystem/shell/D-Bus proxy APIs.
- Capability support определяется runtime evidence/probes, а не DMI model name.
- Read и write evidence независимы.
- `Unsupported`, `BackendMissing`, `TemporarilyUnavailable`, `PermissionDenied`
  и `Unknown` — разные состояния.
- `ApplyResult::Accepted` не означает `Applied`.
- Authoritative observed state обновляется только из read/read-back evidence.
- Неподтверждённая функция остаётся fail-closed в UI, Hardware1 composition,
  policy и sandbox where applicable.
- Live hardware claims всегда revision-scoped.

## 2. Слои

```text
Slint UI / read-only orbisctl
  ↓ typed commands/reads
orbis-ui sequential worker / application services
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
| `orbis-cli` | Read-only Session1 CLI (`orbisctl`) |
| `orbis-test-support` | Test/demo fixtures; remaining GUI release-graph cleanup tracked in #115 |

## 3. Read path

Canonical session path:

```text
authoritative backend
→ orbis-sessiond
→ io.github.orbiscontrol.Session1
→ orbis-session-client/provider
→ application worker / orbisctl
→ GUI or terminal output
```

Session1 reads are fresh and do not become mutation proxies. `orbisctl status`
uses only Session1 Battery/Performance/GPU reads and applies provider-declared
timeouts; it has no Hardware1 mutation command.

Current production read concepts include Battery Charge Limit, Performance,
independent GPU power/MUX/access, profile-specific fan curves, telemetry and
selected ASUS/display diagnostics.

Battery discovery is **lazy/capability-local** in Session1. Absence or temporary
failure of UPower must not prevent independent Performance/GPU/Fan reads.

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

### Current production enablement

Typed backend code or a stable Hardware1 method does **not** imply a shipped
write feature. Current production composition intentionally enables live
mutation backends only for:

- **Performance** — fixed `platform_profile` write + fresh read-back;
- **Battery** — only after fail-closed startup evidence: effective threshold
  discovery, non-activating `NameHasOwner(xyz.ljones.Asusd)`, and readable
  effective kernel threshold.

The following are hard-disabled in production Hardware1 composition and return
`Unsupported` even if a local administrator weakens polkit:

- raw GPU mutation;
- Fan curve/set-defaults mutation;
- Panel Overdrive mutation;
- Keyboard Backlight mutation;
- Aura Static RGB mutation.

Packaged polkit independently denies these five groups by default. The root
service keeps all `/sys` read-only except the one exact enabled direct-write
path `/sys/firmware/acpi/platform_profile`.

Historical live evidence for Performance/Battery is revision-scoped and recorded
in `current-state.md`; it does not automatically validate later revisions.

## 5. Capability registry

`orbis-capabilities` owns immutable runtime snapshots. Each capability carries
separate operation evidence; consumers must gate writes from
`operations.write.status`, not from overall status alone.

Production composition performs read probes, queries typed mutation-status
evidence, builds a whole snapshot, swaps it atomically and publishes generation
changes. Periodic refresh re-queries mutation status before rebuilding. Explicit
refresh must follow the same rule; #112 records the current stale-status defect.

Disabled Hardware1 backends must remain non-writable in downstream UI,
diagnostics and support evidence (#120). Failure in one capability must not
collapse unrelated capabilities.

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
mutation confirms fresh configured/effective state.

Battery power-supply discovery now preserves permission/transient failures and
uses `Unsupported` only for successfully inspected structural absence (#108
source fix). Production Hardware1 does not activate a stopped asusd merely to
probe writability: startup uses D-Bus `NameHasOwner`. Dynamic owner/interface
refresh after startup remains #107/#112.

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
those concepts and defines confirmation semantics. Raw GPU Hardware1 mutation
is code-disabled and policy-denied; raw backend enum values are not a product API.

## 9. Fan contract

There are two distinct read concepts:

- **active curve** from `asus_custom_fan_curve` sysfs;
- **stored profile-specific curve** from asusd `FanCurveData(profile)`.

They must not be conflated. ASUS fan profile identity is lossless
(`Balanced/Performance/Quiet/LowPower`). Quiet and LowPower must not collapse in
mutation/wire state.

Current production write path is hard-disabled in UI + polkit + Hardware1
composition. The dormant compatibility backend still has known issues:

- preserve custom `CurveData.enabled` (#104);
- restore original performance profile even when Factory Defaults fails (#105);
- do not infer CPU/GPU fan support from each other (#109);
- preserve profile-specific `enabled` evidence through Session1/UI (#116).

No fan mutation is re-enabled until these are fixed, executable CI is green and
controlled hardware validation confirms final state.

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

Loading config/defaults must never itself trigger hardware mutation. Legacy
`AppConfig` defaults are now hardware-inert; legacy writes are durable/atomic;
checked XDG resolvers fail instead of choosing CWD. Deprecated fallback helpers
remain compatibility-only pending removal (#113).

## 12. Preferences and XDG state

Production persistence concerns remain separate:

- `preferences.toml` — application presentation/behavior;
- independent window state;
- owned XDG autostart desktop entry;
- `desired-state.toml` — typed desired-state foundation.

New stores use versioning, same-directory atomic replacement, durability and
permission handling. Run on Startup backend exists, but current Rust lifecycle
glue is incomplete; its UI remains disabled until #110 is completed.

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
Workspace/sessiond/session-client default dependency paths now disable mock
features explicitly. The full GUI still carries `orbis-test-support` because
production and screenshot bootstrap share `UiState::from_mock_profile`; removing
that remaining dependency/fixture-derived initial state is #115.

## 15. Runtime timeout boundary

Provider traits expose a timeout, but the sequential GUI application/worker path
does not yet enforce it generically (#123). One stuck provider can therefore
block unrelated queued work. New `orbisctl status` already wraps each read in
its provider timeout; GUI/application enforcement remains a release hardening
item.

## 16. Packaging and release gate

Nix packaging installs GUI/sessiond/CLI, Hardware1 support, D-Bus policy,
per-capability polkit policy, desktop/AppStream metadata and NixOS services.

Canonical checks are lockfile-strict (`cargo ... --locked`) and flake checks now
include support-matrix schema validation plus XML/desktop metadata syntax gates.
Hardware1 lifecycle VM asserts the exact minimal sysfs writable surface and
polkit default matrix without performing valid hardware mutation.

A release candidate requires:

- no known unsafe write path enabled by default;
- executable Cargo/Nix checks on the exact revision;
- green `nix flake check`;
- package/metadata acceptance;
- dated evidence for device-specific claims;
- unsupported/incomplete controls shown honestly as unavailable/unknown.

GitHub Actions currently fails before its first workflow step / fresh pushes may
receive no run (#106). After CI is executable, `main` should be protected with
required checks (#114).

## 17. Source-of-truth hierarchy

Use documents by role:

1. [`current-state.md`](current-state.md) — what is true now;
2. this file — architecture that must hold;
3. ADRs — why stable decisions were made;
4. [`roadmap.md`](roadmap.md) — future ordering;
5. research/audit documents — supporting evidence, not automatic production claims.

When code and a current-design document diverge, update the document or block
the behavior; do not silently reinterpret evidence.
