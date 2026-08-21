# Architecture — Orbis Control

> Роль: **CURRENT DESIGN**.
> Source snapshot: development branch (consolidates the former `asus-hardware-validation-20260821` line).
> Operational readiness — [`current-state.md`](current-state.md), future work — [`roadmap.md`](roadmap.md), stable decisions — [`adr/`](adr/).

## 1. Architectural invariants

- GUI is a normal user-session process. Interactive euid-0 rejection occurs before preferences, runtime, or bus setup; screenshot/offscreen paths remain available for tests.
- `orbis-sessiond` is a read/session boundary, never a privileged mutation deputy.
- Privileged writes go only through typed `Hardware1` methods owned by `orbis-hardwared`.
- No generic root filesystem/sysfs/shell/D-Bus proxy API.
- Capability support comes from runtime evidence, not model-name tables.
- Read and write evidence are independent.
- `Unsupported`, `BackendMissing`, `TemporarilyUnavailable`, `PermissionDenied` and `Unknown` remain distinct.
- `ApplyResult::Accepted` never means `Applied`.
- Authoritative UI state is updated only from reads/read-back, not optimistic local mutation.
- Product-disabled controls remain fail-closed even when implementation code exists.
- Hardware claims are revision- and environment-scoped.

## 2. Layering

```text
Slint UI / read-only status CLI / explicit validation CLI
  ↓ typed requests / observations
orbis-ui runtime + orbis-application services
  ↓
provider traits + capability registry
  ├─ Session1 reads → orbis-sessiond → UPower / kernel / supergfxd / asusd
  ├─ direct read-only providers → sysfs / Wayland observation
  └─ Hardware1 mutations → orbis-hardwared → polkit → typed backend
```

Primary crate ownership:

| Crate | Responsibility |
|---|---|
| `orbis-core` | Domain types, state/action invariants |
| `orbis-config` | XDG preferences/state/autostart/desired-state persistence |
| `orbis-capabilities` | Typed evidence and immutable registry snapshots |
| `orbis-providers` | Platform/provider implementations and bounded call primitive |
| `orbis-application` | Use cases and command/read-back composition |
| `orbis-session-protocol` | Session1 wire contract |
| `orbis-session-client` | Session1 readers and caller-preserving Hardware1 clients |
| `orbis-sessiond` | User-session read daemon |
| `orbis-hardwared` | Narrow privileged service |
| `orbis-ui` | Slint surfaces, production composition, worker runtime |
| `orbis-cli` | Read-only status CLI and explicit Hardware1 validation tool |
| `orbis-test-support` | Fixtures/screenshots/tests; release-graph cleanup remains #115 |

The active production worker is `crates/orbis-ui/src/worker_runtime.rs`. Legacy large worker files are not the source of truth for new runtime fixes.

## 3. Read path

```text
authoritative source
→ Session1 or typed read-only provider
→ bounded provider/application read
→ worker / CLI / diagnostics
→ presentation
```

Current read concepts include:

- Battery charge-limit state;
- Performance current/available profiles;
- independent GPU power, physical MUX and access policy;
- profile-specific fan curves and active fan curves;
- sysfs telemetry;
- selected display/ASUS diagnostics.

Read failures are capability-local. Missing Battery/UPower must not collapse unrelated GPU/Performance/Fan reads.

Provider operations are bounded and timeout results are capability-local. A
timeout maps to `TemporarilyUnavailable`, while invalid data remains `Unknown`,
permission failures remain `PermissionDenied`, and structural backend absence
does not become a timeout or `Unsupported` by guesswork.

`orbisctl status` and `status --json` use only read providers. The JSON schema is versioned and carries explicit observation states rather than fake values.

## 4. Privileged mutation path

```text
original GUI/application caller
→ Hardware1 system bus
→ per-capability polkit
→ narrow typed backend
→ authoritative read-back / explicit Pending / failure
```

Current product enablement is intentionally narrow:

- **Performance** — platform-profile mutation with read-back;
- **Battery** — conditional typed mutation after fail-closed ownership/effective-threshold evidence.

Production `Hardware1` composition keeps these writes disabled:

- raw GPU mutation;
- Fan set/reset;
- Panel Overdrive;
- Keyboard Backlight;
- Aura Static RGB.

Typed code existence does not override this policy. Packaged polkit and the service sandbox provide additional defense-in-depth. The intended direct sysfs writable surface of `hardwared` is only `/sys/firmware/acpi/platform_profile` until separately promoted controls are validated.

### Privileged backend activation lifecycle

The service binaries, service contracts and activation policy are separate
artifacts. Their presence in the repository or in a package does not mean that
either daemon is running.

#### Development

- `services.orbis-control.enable` defaults to `false`; no `orbis-sessiond` user
  unit or `orbis-hardwared` system unit is enabled by default.
- `nixosModules.orbis-hardwared-policies` is a **policy-only** integration. It
  installs static D-Bus policy and polkit action files, but does not create or
  start a daemon.
- The standalone `orbis-hardwared` package and
  `packaging/deploy-dev-hardwared.sh` provide a separate development lifecycle.
  This path must not be combined with the NixOS-managed unit because both use
  the same `Hardware1` bus name.
- Tests use fake sysfs/P2P boundaries and do not start host daemons.

#### Production

Enabling `services.orbis-control` in the NixOS module is the production
activation mechanism. The module then:

- starts `orbis-sessiond` as a user-session D-Bus service at
  `graphical-session.target`;
- starts `orbis-hardwared` as a root system D-Bus service after `dbus.service`;
- uses the packaged `orbis-sessiond` and `orbis-hardwared` binaries;
- installs the Hardware1 system-bus policy and per-capability polkit actions;
- applies the narrow hardwared sandbox, with only
  `/sys/firmware/acpi/platform_profile` writable.

The flake exposes the full `orbis-control` package, a standalone
`orbis-hardwared` package, and a policy-only package. The full package source
contains the workspace binaries used by the module, while the standalone
package intentionally builds only `orbis-hardwared` and does not define a
systemd unit.

The FA707NV `Hardware1 unavailable` result is therefore classified as a
deployment/lifecycle gap, not as missing Rust implementation or an intentional
product capability disablement: the crate, package recipe, NixOS unit,
Hardware1 D-Bus contract and polkit action exist, but the host has no enabled or
installed Orbis service, no owned Hardware1 bus name, and no activatable service
registration. A policy file alone cannot activate the daemon or provide a
write path.

## 5. Capability registry

The registry is immutable and generation-based. Each capability carries operation-level evidence; mutation controls must gate from `operations.write.status` or an equivalent typed product-write status, not overall capability presence.

Refresh contract:

1. re-query mutation-status evidence;
2. execute read-only capability probes;
3. build a complete next-generation snapshot;
4. publish by whole-snapshot replacement.

Explicit and periodic refresh now use the same canonical path (#112 source-complete).

Public probes use bounded adapters. A probe timeout is local `TemporarilyUnavailable`, not `Unsupported`, and should not abort unrelated discovery.

## 6. Bounded execution

`orbis_providers::bounded_provider_call()` owns provider-declared read deadlines and performs no retry.

Current source coverage includes:

- public capability probes;
- `orbisctl` reads;
- worker Battery refresh;
- worker Performance refresh and Automation recovery read;
- worker Fan curve refresh;
- independent GPU power/MUX/access refreshes.

GPU primitive reads run concurrently with separate provider deadlines, so one hung concept does not serially block the other two.

Remaining #123 boundary:

- telemetry must expose/use a canonical deadline path;
- Hardware1 mutation-status requery is bounded (`HARDWARE1_STATUS_DEADLINE`, timeout → `Unknown`); executable validation of the exact revision remains;
- mutation timeout after possible dispatch is **unknown outcome** and must enter observation/recovery, never generic blind retry.

## 7. Battery model

```rust
ChargeLimit {
    enabled: bool,
    configured_percent: Option<Percent>,
    effective_percent: Option<Percent>,
    bounds: Option<ChargeLimitBounds>,
}
```

Sources remain independent:

- `enabled` — policy/backend state;
- configured threshold — backend configuration;
- effective threshold — kernel observation;
- bounds — only when actually reported/proven.

Wire payloads are strict-decoded; absent values are not replaced by defaults. Production startup avoids activating a stopped asusd simply to prove writability. Dynamic owner/interface liveness remains #107.

## 8. Performance model

Read and write remain distinct:

```text
read:  Session1 → platform_profile / choices
write: original caller → Hardware1 → polkit → fixed path → read-back
```

Unknown wire values are protocol failures, not guessed profiles. UI selection is authoritative only after a successful read/read-back.

## 9. GPU model

These are separate concepts:

```text
physical MUX
!= dGPU access policy
!= runtime dGPU power
!= requested product GPU mode
!= pending reboot/logout requirement
```

Production supports the first three as independent reads. Eco/Standard/Ultimate/Optimized remain product policy, not aliases for a raw backend enum.

## 10. Fan model

Two different observations exist:

- active curve from sysfs;
- stored profile-specific curve from asusd/Session1.

Per-fan profile reads now target a concrete `(profile, fan)`; the old requirement that one requested fan read must contain both CPU and GPU is no longer true in this branch.

Remaining evidence problems:

- aggregate `FeatureId::FanCurves` is still derived from a CPU probe and can overstate GPU support (#109);
- stored custom-curve `enabled` is not carried through the final typed observation (#116).

Writes stay hard-blocked until #104/#105/#109/#116 and exact-build executable/live validation are complete.

## 11. Telemetry

`SysfsTelemetryProvider` is read-only and dynamically discovers hwmon/power-supply sources. Partial metrics are allowed; unsupported derived values are not invented.

Freshness must mean recent useful observation, not merely `Ok(Telemetry)`. Empty/partial/field-local failure evidence remains #117.

## 12. Desired / Observed / Pending

```text
Desired  = explicit persisted/user/policy intent
Observed = authoritative runtime evidence
Pending  = unconfirmed transition + requirement/recovery state
```

Loading configuration must never itself trigger hardware mutation. Reconciliation is not a generic “apply config” loop; it must compare fresh Observed state, evaluate capability/policy, perform one deliberate action, then read back.

Legacy config defaults are hardware-inert and new stores use checked XDG paths. Deprecated compatibility helpers remain #113.

Pure research-foundation modules (preset/policy selection, reconciliation decisions, transaction phases, readiness, software fan-policy computations) exist in `orbis-core`/`orbis-config`/`orbis-providers` but no production executor consumes them yet; they remain FOUNDATION-only until a deliberate wiring step preserves the separation above.

## 13. Preferences / desktop lifecycle

Separate stores/owners remain separate:

- `preferences.toml` — application behavior/presentation;
- window-state file — position only;
- owned XDG autostart desktop entry — startup source of truth;
- `desired-state.toml` — hardware intent foundation.

Source wiring now includes:

- Autostart read/write/read-back (#110 closed);
- Start Minimized persistence;
- Remember Position on supported X11-style sessions;
- explicit Wayland fail-closed positioning;
- StatusNotifier tray lifecycle;
- `HideToTray` only with a live host;
- explicit Quit event-loop termination (#121 closed).

These are user-level lifecycle settings, not Desired hardware state.

## 14. Diagnostics

Diagnostics is read-only and privacy-bounded:

```text
allowlisted typed sources
→ DiagnosticsSnapshot
→ DiagnosticsUiDto
→ UI/text/JSON projection
```

The active branch initializes `DiagnosticsRuntime`, tracks capability generation replacement, performs one-shot Refresh off the Slint callback thread, and supports privacy-bounded Export/Copy (#111 closed). Open Logs remains intentionally disabled until a safe typed host-opening contract exists.

## 15. Automation

The current worker owns policy revision, lifecycle observations, AC/Battery debounce, resume freshness/coalescing, capability-generation preflight, replay-resistant serialization and Performance unknown-outcome recovery.

After a paired logind resume observation, the worker refreshes authoritative read-only provider state and publishes a new capability generation. Resume is not a mutation trigger: desired presets are not restored, and automation observation is restricted to dry-run preparation on this path.

A Performance-only executor contract exists with mandatory authoritative read-back, but `AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED` remains `false`. GPU/Fan/Battery/Display/Lighting unattended executors remain disabled.

## 16. Display / Updates / ambiguous controls

Display Refresh has typed target/request/read-back semantics but no concrete compositor mutation owner. No shell fallback is accepted.

Updates can classify installation ownership/blockers but has no canonical signed release feed or universal installer owner. It must not invent package-manager/self-replacement behavior.

Ambiguous ASUS controls stay disabled until exact upstream/domain ownership and confirmation semantics are proven.

## 17. Mock/test boundary

Mock/fixture data is test/development evidence only. Production cannot treat fixture values as hardware observations.

The remaining release-graph defect is #115: the GUI still has a normal `orbis-test-support` dependency because screenshot and production bootstrap share fixture-derived `UiState` construction. The intended fix is a production-native Loading/Unknown/non-writable constructor plus dev/test-only fixture construction.

## 18. Release boundary

Executable release claims require the exact candidate revision to pass Rust/Slint/Nix/package validation. Static contracts are useful fail-fast checks, not compilation evidence.

Current blockers include #106, #125, remaining #123 work, #124 identity and #114 main protection after CI recovery. Fan/GPU/extended writes remain blocked by their separate evidence gates.
