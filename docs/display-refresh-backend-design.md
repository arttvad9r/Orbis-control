# DisplayRefresh backend design

Status: typed evidence, request, authoritative applied-state, provider-owner and application orchestration contracts exist; no production compositor mutation implementation exists. FA707NV live probe (2026-08-25) found the KScreen/KWayland runtime owner unavailable: `plasma-kscreen.service` is active and `kscreen-doctor` observes `eDP-2`, but the D-Bus loader returns an empty backend and logs `Could not find slot BackendLoaderAdaptor::requestBackend`.

## Invariant

Display refresh mutation is a **session/compositor concern**, not an ASUS firmware write and not a reason to extend `orbis-hardwared`.

The existing `WaylandDisplayOutputProvider` remains read-only. Core `wl_output` gives useful authoritative observation, but it is intentionally insufficient for mutation:

- `wl_output.name` is runtime identity and clients must not assume it maps to an underlying DRM connector;
- non-current modes are deprecated and a compositor may advertise only the current mode;
- standard core Wayland exposes no universal output modeset operation.

Therefore Orbis must not infer a writable panel from a name such as `eDP-1`, must not infer missing 60/120 modes as unsupported, and must not add a shell fallback such as `wlr-randr`, `kscreen-doctor`, `xrandr` or an arbitrary command invocation.

## Typed model

### Target and supported-preset evidence

`DisplayRefreshEvidence` contains:

- an opaque target ID supplied by a compositor-specific owner;
- owner-proven target role (`InternalPanelProven`, `External`, `Unknown`);
- current compositor mode;
- exact product preset targets derived from owner evidence.

Fixed targets retain exact mHz. A 60-Hz product preset may therefore refer to 59.94 Hz rather than rewriting it to 60.00 Hz.

Only same-resolution modes are candidates for a refresh-only preset. A 120-Hz mode at a different resolution is not silently treated as a refresh target.

`Auto` support is never inferred from core `wl_output`; a compositor mutation owner must prove that Auto is an operation it actually owns.

### Capability-safe constraints

`DisplayRefreshConstraints` deliberately drops current observed state. It keeps only:

- opaque owner target identity;
- proven target role;
- exact supported Auto/60/120 product targets.

This prevents capability metadata from becoming a second observed-state store.

### Validated request

`DisplayRefreshRequest` has private fields and can be constructed only from `DisplayRefreshConstraints::writable_target_for()` evidence. It preserves the opaque owner target and exact preset target. Callers cannot freely combine an arbitrary output name with an arbitrary frequency.

A request is still not lasting authority: topology and available modes can change after construction.

### Authoritative applied state

Supported-target evidence is not enough for post-write success. In particular, a current 59.94/60/120-Hz mode does **not** prove whether an automatic refresh policy is active.

`DisplayRefreshAppliedState` therefore carries a separate authoritative active policy:

- `DisplayRefreshActivePolicy::Auto` — owner positively reports automatic policy active;
- `Fixed { refresh }` — owner positively reports a fixed policy at the exact lossless mHz value;
- `Unknown` — current mode may be observable, but active Auto-vs-fixed policy is not proven.

`Unknown` never confirms mutation success.

## Provider boundary

`DisplayRefreshMutationOwner` is the compositor-adapter contract. It exposes three independent operations:

1. `display_refresh_evidence()` — fresh target/role/supported-preset evidence;
2. `set_display_refresh(&request)` — one bounded typed mutation using a **borrowed** validated request;
3. `display_refresh_applied_state()` — fresh authoritative post-operation current mode and active policy.

The owner must re-read its own evidence and call `validate_display_refresh_request()` immediately before mutation under its topology/serialization boundary. Revalidation fails closed if:

- the target is no longer proven internal;
- opaque target identity changed;
- exact preset evidence changed.

`validate_display_refresh_readback()` performs the post-write proof. It requires:

- the same opaque target;
- target still positively proven internal;
- resolution unchanged;
- Auto request → authoritative active policy exactly `Auto`;
- 60/120 request → active policy exactly `Fixed` at the exact requested mHz and current mode at that same mHz.

A provider `ApplyResult` alone is never UI/Automation success.

No production implementation currently implements this trait.

## Application orchestration

`orbis-ui::display_refresh_service::apply_display_refresh()` develops the application-level command semantics without choosing a compositor implementation.

The sequence is:

1. application fresh `display_refresh_evidence()` read;
2. application-level stale-request validation;
3. owner mutation call (whose own contract requires a second just-in-time evidence revalidation);
4. mandatory `display_refresh_applied_state()` read;
5. exact `validate_display_refresh_readback()`;
6. only then return `DisplayRefreshCommandOutcome`.

Failures remain distinguishable:

- `PreflightRead` — no fresh evidence, mutation not attempted;
- `StaleRequest` — request no longer matches fresh evidence, mutation not attempted;
- `Command` — owner mutation failed before an `ApplyResult` existed;
- `ReadBack` — owner returned an `ApplyResult`, then authoritative read failed; outcome is **unknown**;
- `ReadBackMismatch` — authoritative state was read but does not prove the exact request.

The outcome preserves the original `ApplyResult`. An unattended executor may impose the stricter rule that only `Applied` is success; `Accepted` is not automatically promoted.

## Automation preflight

The compatibility `preflight_automation_plan(plan, capabilities)` intentionally supplies no Display owner constraints. Consequently Display actions remain blocked even if a generic `FeatureId::DisplayRefresh.write=Supported` were accidentally published.

`preflight_automation_plan_with_display_constraints()` is the future owner-aware overload. It requires all of:

- `FeatureId::Automation.write == Supported`;
- `FeatureId::DisplayRefresh.write == Supported`;
- an exact product mapping (`Auto`, fixed 60, fixed 120 only);
- frozen typed DisplayRefresh constraints;
- `InternalPanelProven` target role;
- the requested exact preset target present in those constraints.

`Minimum`, `Maximum`, arbitrary fixed refresh rates and missing/unknown target evidence remain blocked. Production currently has no caller supplying Display constraints.

## Adapter strategy

There is no universal standard-Wayland mutation transport. Implementations should be independent adapters behind the same typed owner contract. Candidate families must be validated separately, for example:

- wlroots compositors through an output-management protocol when explicitly advertised;
- KDE through a stable supported KScreen/session interface if validated;
- GNOME through a stable supported Mutter DisplayConfig interface if validated.

Do not select an adapter only from environment strings. Runtime service/protocol discovery and exact version/operation evidence are required.

The current dependency graph contains `wayland-client 0.31` for core output observation. A new output-management protocol binding would change the dependency graph/lockfile and must be added only with real Cargo tooling rather than hand-editing `Cargo.lock` in the current no-Cargo sandbox.

## Promotion gate

Display Quick Control write must remain disabled until all of the following are true:

1. one concrete compositor adapter has executable typed tests;
2. adapter discovery does not guess compositor or output target;
3. internal-panel identity is positively proven;
4. exact 60/120/Auto targets are proven independently;
5. mutation revalidates the request against fresh evidence immediately before the write;
6. operation has a bounded timeout and defined unknown-outcome behavior;
7. owner can publish authoritative active Auto-vs-fixed policy after the write;
8. application orchestration performs mandatory post-write read-back;
9. `Unknown` active policy, read failure, target drift, resolution change or policy mismatch never reports success;
10. operation-level DisplayRefresh capability is assembled from the concrete owner rather than read-only `wl_output` presence;
11. Quick Controls set `display-control-ready=true` only from that evidence;
12. Rust/Slint executable validation passes.

Until then the current UI behavior is correct: observed refresh is shown, but mutation controls remain disabled. Installing `supergfxd` does not address this compositor/session owner boundary.
