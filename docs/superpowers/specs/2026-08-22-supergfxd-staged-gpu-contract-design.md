# Staged supergfxd GPU Contract

## Status

Proposed design for the first product-GPU preparation slice. This document
does not enable live GPU mutation and does not map product presets to backend
modes.

## Goal

Define one typed, testable contract for the `supergfxd` staged lifecycle so
Orbis can later reason about `Hybrid <-> Integrated` transitions without
mistaking a request, a pending transition, or a required logout/reboot for an
applied GPU state.

## Scope

Included:

- exact `supergfxd` backend mode and user-action wire decoding;
- read-only staged snapshot acquisition over a typed D-Bus client;
- one request/read-back classification flow for `Hybrid` and `Integrated`;
- private P2P fake-service tests for applied, pending, user-action, failure and
  inconsistent states;
- explicit preservation of `Applied`, `Pending`, `RequiresUserAction` and
  `Inconsistent` results.

Excluded:

- production `Hardware1` GPU write promotion;
- UI mutation controls and automatic logout/reboot;
- `AsusMuxDgpu`, `NvidiaNoModeset`, VFIO and eGPU transitions;
- product mapping for `Eco`, `Standard`, `Ultimate` and `Optimized`;
- reimplementation of display-manager, driver, PCI or runtime-PM lifecycle;
- real hardware writes or validation on the host.

## Ownership and boundaries

`supergfxd` remains the compatibility owner of the complete GPU lifecycle. Orbis
does not stop display managers, kill processes, unload drivers, manipulate PCI
devices, or write GPU-related sysfs paths.

The future privileged path remains:

```text
original application caller
  -> typed Hardware1
  -> capability-specific polkit
  -> orbis-hardwared
  -> typed supergfxd system D-Bus client
  -> supergfxd staged lifecycle
```

This slice stops before the first two Orbis mutation boundaries. It may use a
private P2P fake service in tests, but must not call the real system bus or real
`supergfxd` from automated tests.

## Data model

The backend snapshot contains:

- `current_mode`;
- `pending_mode`;
- `pending_user_action`;
- `power`;
- `supported_modes`.

Unknown future wire values are preserved as explicit `Unknown(value)` and are
never coerced to a known mode or power state.

The request result contains:

- requested backend mode;
- returned `UserActionRequired`;
- fresh post-request snapshot;
- pure staged classification;
- any transport/protocol error.

The returned action is advisory evidence only. It never overrides the fresh
snapshot and never means that the requested mode is applied.

## Classification rules

For requested mode `R` and fresh snapshot `S`:

| Condition | Classification |
|---|---|
| `current == R`, `pending == None`, action `Nothing` | `Applied` |
| `current != R`, `pending == R`, action `Nothing` | `Pending` |
| `current != R`, `pending == R`, action != `Nothing` | `RequiresUserAction(action)` |
| unknown values, pending mismatch, action without pending, or current/pending contradiction | `Inconsistent` |

`Inconsistent` is a recovery-required state. The client must not retry the
request automatically or infer a rollback.

## Request flow

1. Read `Supported` and validate that the requested backend mode is advertised.
2. Read a fresh pre-request snapshot.
3. Send exactly one typed `SetMode` request.
4. Read `Mode`, `PendingMode`, `PendingUserAction`, `Power` and `Supported`
   again.
5. Classify the result using the table above.
6. Return the typed result or the original error class.

Unsupported or unknown requested modes fail before `SetMode`. Set-mode errors
and read-back errors are returned without retry. A timeout after dispatch is an
unknown outcome and must not be converted into success.

## Tests

The private P2P fake service must cover:

- immediate `Applied` transition;
- `Logout` requirement;
- `Reboot` requirement;
- `Pending` with no user action;
- unsupported request rejected before `SetMode`;
- set-mode D-Bus failure with exactly one request;
- read-back failure with exactly one request;
- contradictory current/pending/action state;
- future wire values preserved and classified fail-closed;
- returned action contradicting the fresh snapshot.

No test may use the host system/session bus, real UPower/asusd/supergfxd, or a
valid GPU mutation.

## Promotion gates

This slice is `IMPLEMENTED/TESTED` only after the exact P2P tests and targeted
Cargo checks pass. It does not change production capability status, polkit,
Hardware1 composition, UI writability, NixOS sandbox, or product-mode mapping.

Live promotion requires a separate design and evidence for:

- exact installed `supergfxd` version and owner availability;
- original-caller authorization through Hardware1/polkit;
- logout/reboot presentation and lifecycle policy;
- authoritative post-transition read-back;
- controlled hardware validation with final GPU state proof.
