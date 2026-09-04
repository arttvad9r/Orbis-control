# Architecture — Orbis Control

This document describes durable system boundaries and semantics. It is not a status report, backlog, release checklist, or instruction to preserve obsolete implementation details. Current work is driven by production source, executable behavior, and `TODO.md`.

## 1. System boundaries

Orbis is split into three trust domains:

```text
normal user UI / CLI
        ↓
application + user-session services
        ↓
privileged typed hardware service (only where required)
```

### GUI

`orbis-ui` is a normal user-session application. It owns presentation, interaction, runtime composition and user-visible state. It must not perform direct privileged hardware writes.

### Session service

`orbis-sessiond` owns user-session/read-oriented integration that is better isolated behind D-Bus. It is not a privileged mutation deputy.

### Privileged service

`orbis-hardwared` owns the narrow privileged mutation surface. Its API is semantic and typed. It must never become a generic root filesystem, sysfs, shell-command or arbitrary D-Bus proxy.

## 2. Crate ownership

| Crate | Responsibility |
|---|---|
| `orbis-core` | Domain types, state semantics and invariants |
| `orbis-config` | Preferences, persisted intent and XDG state |
| `orbis-capabilities` | Runtime capability evidence and snapshots |
| `orbis-providers` | Platform/backend traits and concrete implementations |
| `orbis-application` | Use cases and application-level orchestration |
| `orbis-session-protocol` | Session D-Bus wire contract |
| `orbis-session-client` | Session readers and Hardware1 client composition |
| `orbis-sessiond` | User-session daemon/read boundary |
| `orbis-hardwared` | Narrow privileged mutation service |
| `orbis-ui` | Slint UI, runtime composition and worker execution |
| `orbis-cli` | Command-line inspection/diagnostic entry points |
| `orbis-test-support` | Development/test fixtures only |

The current UI worker implementation is `crates/orbis-ui/src/worker_runtime.rs`. Slint source lives under `ui/`.

## 3. Read path

A normal read flows from an authoritative or explicitly best-effort platform source into a typed provider and then upward:

```text
kernel / sysfs / UPower / asusd / supergfxd / compositor API / other typed source
→ provider
→ application or session boundary
→ runtime worker
→ UI / CLI / diagnostics
```

Rules:

- failures stay local to the affected capability whenever possible;
- absence, unsupported hardware, permission failure, temporary backend failure and malformed data are distinct states;
- unknown values remain unknown rather than being replaced with product defaults;
- independent concepts are read independently rather than inferred from one another;
- time-bounded provider reads must not turn a timeout into false `Unsupported` evidence.

## 4. Mutation path

Privileged mutations use a bounded semantic path:

```text
original application caller
→ typed Hardware1 request
→ capability-specific authorization
→ narrow backend operation
→ authoritative read-back, explicit pending state, or honest failure
```

A feature may use an unprivileged owner when the platform already exposes a safe user-session API; the same semantic rules still apply.

Mutation invariants:

- validate input at the owning boundary;
- never accept arbitrary caller-controlled privileged paths or commands;
- preserve the original caller identity through authorization where the architecture requires it;
- perform one deliberate mutation attempt;
- do not present requested state as observed state;
- `ApplyResult::Accepted` means the request was accepted, not that physical state was proven applied;
- if transport times out after dispatch may have occurred, the result is unknown and must not be blindly retried;
- when authoritative confirmation is available, read it back before showing success;
- when confirmation requires reboot/logout/later observation, represent that explicitly as pending.

## 5. Capability model

Capability evidence is runtime evidence, not a static model-name allowlist.

Read and write support are separate dimensions. A readable value, an existing backend method, or a known ASUS model does not by itself prove that mutation is safe and supported.

Useful states remain semantically distinct, including:

- supported;
- read-only;
- unsupported;
- backend missing;
- temporarily unavailable;
- permission denied;
- unknown.

The UI should derive control availability from the evidence for the actual operation, not from broad feature presence.

Capability snapshots may be generation-based so a refresh can publish one coherent state rather than exposing partially updated observations.

## 6. Desired, Observed and Pending

Hardware intent and actual state are deliberately separate:

```text
Desired  = what the user or policy wants
Observed = authoritative runtime evidence
Pending  = a requested transition that is not yet conclusively observed
```

Loading configuration is inert. It must not mutate hardware merely because persisted values exist.

Any reconciliation/execution flow should:

1. obtain sufficiently fresh Observed state;
2. compare it with Desired state;
3. verify capability and policy for the concrete operation;
4. execute at most the intended mutation;
5. observe again;
6. publish Applied/Pending/Unknown/Error truthfully.

## 7. Product concepts must stay separate

Do not collapse distinct platform concepts simply because one backend exposes similar enum values.

Examples include:

- GPU product policy vs physical MUX vs dGPU access policy vs runtime power;
- stored fan profile vs currently active fan curve;
- configured battery threshold vs effective kernel-observed threshold;
- requested display mode vs compositor-observed mode;
- accepted configuration vs confirmed physical effect.

This separation belongs in domain/application semantics, not in ad-hoc UI conditionals.

## 8. UI architecture

Slint is the presentation layer. UI callbacks express user intent; Rust runtime/application code owns system behavior.

A user-visible control is considered connected only when its path reaches real production behavior and returns real state/error semantics. Mock-only or fixture-only behavior must not be presented as device state in a production build.

The UI may hide or disable unsupported controls, but it must not simulate successful actions.

## 9. Provider and backend design

Prefer typed provider traits around platform concepts rather than leaking concrete sysfs paths, D-Bus object layouts or vendor-specific values into domain/UI code.

Concrete backends may evolve or be replaced without changing the user-facing meaning of the operation.

When upstream semantics are uncertain:

- inspect official/upstream APIs and existing project evidence;
- preserve uncertainty in the type/state model;
- keep the unsafe/unsupported path fail-closed;
- do not create guessed behavior solely to make a feature appear complete.

## 10. Timeouts and concurrency

External providers can hang or disappear. Reads that can block should have bounded execution appropriate to the provider.

Independent reads may execute concurrently so one failed source does not unnecessarily serialize unrelated state collection.

Retry policy is operation-specific. Generic infrastructure must never automatically retry a mutation whose first attempt may already have reached hardware.

## 11. Persistence

Persist only user/application intent and presentation state that has a clear owner.

Preferences, window state, autostart and hardware Desired state are different concerns and should not be conflated into one implicit "apply everything" configuration object.

Persistent data should use checked XDG locations, deterministic schemas where needed, and safe replacement/write behavior.

## 12. Packaging and service ownership

Declarative package/module source owns installed binaries, desktop/AppStream metadata, D-Bus policy, polkit policy and system/user service definitions.

Do not edit generated system files as the source of truth.

Development helpers and production service ownership must not compete for the same D-Bus name or privileged resource simultaneously.

A package containing a daemon does not by itself prove the daemon is enabled/running; runtime capability discovery must handle service absence honestly.

## 13. Testing boundary

Default automated tests must not mutate the developer's real ASUS hardware.

Use, as appropriate:

- ordinary Rust unit/behavior tests;
- mock providers and fixtures;
- private P2P D-Bus tests;
- fake sysfs/filesystem state;
- NixOS VM integration tests.

These can prove software behavior, protocol wiring, service policy and packaging integration. They do not prove physical device behavior.

Live hardware validation is device/revision specific and should be recorded separately when it is actually performed. Lack of live hardware does not block implementing and testing the software path with honest capability gating.

## 14. Security properties worth preserving

- GUI does not need root privileges.
- Privileged surface remains narrow and typed.
- Untrusted D-Bus/wire values are validated before entering trusted domain state.
- No arbitrary privileged command/path execution API.
- Unsupported and uncertain operations fail closed.
- Unknown mutation outcomes are not silently converted to success or retried.
- Test/mock state cannot escape into normal production state.

Everything outside these durable boundaries may be simplified, refactored or removed when doing so makes the product easier to finish and maintain.