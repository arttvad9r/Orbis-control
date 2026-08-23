# ASUS Product GPU Mutation

## Goal

Add a separate typed `Hardware1.SetProductGpuMode` operation for the ASUS
Armoury three-mode model without changing the existing raw `SetGpuMode`
supergfxd contract.

## Supported modes

```text
Hybrid     -> dgpu_disable=0, gpu_mux_mode=1
Integrated -> dgpu_disable=1, gpu_mux_mode=1
Ultimate   -> dgpu_disable=0, gpu_mux_mode=0
```

`Optimized` is not exposed because the ASUS Armoury GPU API has no such mode.

## Ownership

`asusd` owns the ASUS Armoury firmware attributes. Orbis sends only typed
semantic values through a separate Hardware1 method. It never accepts paths,
attribute names, arbitrary values, shell commands, or generic D-Bus forwarding.

GPU attribute writes are deferred by asusd and applied during shutdown; the
operation therefore returns queued/read-back state and `RebootRequired`, never
an immediate live-applied claim.

## Operation contract

1. Read both current attributes.
2. Require a complete, known, non-conflicted current pair.
3. Authorize the original system-bus caller with a separate capability-specific
   polkit action.
4. Set both typed ASUS attributes in deterministic order.
5. Read current and queued values for both attributes.
6. Return `AlreadyActive`, `Queued`, `RebootRequired`, `Unknown`, or
   `Inconsistent` according to the read-back.

If the first setter succeeds and the second fails, or a timeout may have
occurred after dispatch, return unknown/inconsistent evidence and never retry
automatically. Do not attempt rollback or reboot automatically.

## Boundaries

- Existing raw `Hardware1.SetGpuMode` and supergfxd adapter remain unchanged.
- No automatic shutdown, logout, reboot, display-manager, driver, PCI, or
  runtime-PM operation is added.
- Production enablement requires the new backend, typed D-Bus method, polkit
  action, and UI gate to agree; default remains fail-closed until those are
  validated.
- Tests use private P2P fake asusd and hardwared boundaries only. No valid GPU
  mutation is performed in automated tests.

## Promotion gate

Before the first real call, record a fresh read-only snapshot of both current
and queued attributes, confirm the intended target and explain that the next
shutdown/reboot applies the queued firmware state. Real hardware validation is
separate evidence and must be explicitly authorized.
