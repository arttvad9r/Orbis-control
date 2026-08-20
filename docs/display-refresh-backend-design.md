# DisplayRefresh backend design

Status: typed evidence/request/provider contracts exist; no production mutation implementation exists.

## Invariant

Display refresh mutation is a **session/compositor concern**, not an ASUS firmware write and not a reason to extend `orbis-hardwared`.

The existing `WaylandDisplayOutputProvider` remains read-only. Core `wl_output` gives useful authoritative observation, but it is intentionally insufficient for mutation:

- `wl_output.name` is runtime identity and the protocol says clients must not assume it reflects an underlying DRM connector;
- non-current modes are deprecated and a compositor may advertise only the current mode;
- standard core Wayland exposes no universal output modeset operation.

Therefore Orbis must not infer a writable panel from a name such as `eDP-1`, must not infer missing 60/120 modes as unsupported, and must not add a shell fallback such as `wlr-randr`, `kscreen-doctor`, `xrandr` or an arbitrary command invocation.

## Typed model

`orbis-core::display_refresh` separates three concepts.

### Observed evidence

`DisplayRefreshEvidence` contains:

- an opaque target ID supplied by a future compositor-specific owner;
- owner-proven target role (`InternalPanelProven`, `External`, `Unknown`);
- current compositor mode;
- exact product preset targets derived from evidence.

Fixed targets retain exact mHz. A 60-Hz product preset may therefore refer to an exact 59.94-Hz mode rather than rewriting it to 60.00 Hz.

Only same-resolution observed modes are candidates for a refresh-only preset. A 120-Hz mode at a different resolution is not silently treated as a refresh target.

`Auto` is never inferred from `wl_output` observation. It exists only when a future mutation owner explicitly proves an automatic policy operation.

### Capability-safe constraints

`DisplayRefreshConstraints` deliberately drops current observed mode. It keeps only:

- opaque owner target identity;
- proven target role;
- exact supported Auto/60/120 product targets.

This prevents capability metadata from becoming a second observed-state store.

### Validated request

`DisplayRefreshRequest` has private fields and can be constructed only from `DisplayRefreshConstraints::writable_target_for()` evidence. It preserves the opaque owner target and exact preset target. Callers cannot freely combine an arbitrary output name with an arbitrary frequency.

A request is still not lasting authority: display topology can change after construction.

## Provider boundary

`DisplayRefreshMutationOwner` is the future compositor adapter contract.

It exposes:

1. `display_refresh_evidence()` — fresh authoritative owner evidence;
2. `set_display_refresh(request)` — one bounded typed mutation.

`validate_display_refresh_request()` revalidates an existing request against a fresh evidence snapshot immediately before mutation. It fails when:

- the target is no longer proven internal;
- opaque target identity changed;
- exact preset evidence changed.

A concrete provider must perform the evidence read and validation under its own topology/serialization boundary. A successful provider `ApplyResult` is not enough for UI success: the future application service must read `display_refresh_evidence()` again and confirm the resulting current policy/mode.

No production implementation currently implements this trait.

## Automation preflight

The compatibility `preflight_automation_plan(plan, capabilities)` intentionally supplies no Display owner constraints. Consequently Display actions remain blocked even if a generic `FeatureId::DisplayRefresh.write=Supported` were accidentally published.

`preflight_automation_plan_with_display_constraints()` is the future owner-aware overload. It requires all of:

- `FeatureId::Automation.write == Supported`;
- `FeatureId::DisplayRefresh.write == Supported`;
- an exact product mapping (`Auto`, fixed 60, fixed 120 only);
- frozen typed DisplayRefresh constraints;
- `InternalPanelProven` target role;
- the requested exact preset target present in those constraints.

`Minimum`, `Maximum`, arbitrary fixed refresh rates and missing/unknown target evidence remain blocked. This path is currently exercised only by synthetic tests; production has no caller supplying Display constraints.

## Adapter strategy

There is no universal standard-Wayland mutation transport. Implementations should be independent adapters behind the same typed owner contract. Candidate families must be validated separately, for example:

- wlroots compositors through an output-management protocol when explicitly advertised;
- KDE through a stable supported KScreen/session interface if one can be validated;
- GNOME through a stable supported Mutter DisplayConfig interface if one can be validated.

Do not select an adapter only from environment strings. Runtime service/protocol discovery and exact version/operation evidence are required.

The current dependency graph contains `wayland-client 0.31` only for core `wl_output` observation. It does not currently contain a typed output-management protocol binding. Adding a new protocol crate must update the lockfile under real Cargo tooling; that should not be hand-edited in the current no-Cargo sandbox.

## Promotion gate

Display Quick Control write must remain disabled until all of the following are true:

1. one concrete compositor adapter has typed executable tests;
2. adapter discovery does not guess a compositor or output target;
3. internal-panel identity is positively proven;
4. exact 60/120/Auto targets are proven independently;
5. mutation revalidates the request against fresh evidence immediately before the write;
6. operation has a bounded timeout and defined unknown-outcome behavior;
7. application layer performs authoritative post-write read-back;
8. read-back mismatch never reports success;
9. topology change during the operation fails closed;
10. operation-level DisplayRefresh capability is assembled from the concrete owner rather than from read-only `wl_output` presence;
11. Quick Controls set `display-control-ready=true` only from that evidence;
12. Rust/Slint executable validation passes.

Until then the current UI behavior is correct: observed refresh is shown, but mutation controls remain disabled.
