# ASUS FA707NV live validation — 2026-08-25

> Scope: **revision-scoped LIVE-VALIDATED evidence** for the `development`
> integration line at commit **`7db695c437a8649e1b40bbbfa6c8f1f081ba058e`**
> (deployed store package `96xcnyn6paqk75jkbz4s99hs984vnsr2-orbis-control-0.1.0`
> via the canonical NixOS module `services.orbis-control.enable = true`).
>
> Host: ASUS TUF Gaming A17 FA707NV (`/sys/class/dmi/id/product_name`), NixOS,
> real `asusd` active, UPower active, user session bus active.
> Authorization: explicit owner approval for live hardware work (session log).
> All mutations were performed through the production Hardware1 system-bus path
> (original caller → polkit → typed backend → authoritative read-back) and
> every mutation was restored to its pre-test value.

## Deployment provenance

- Previous deployment was stale: lock rev `4d29ed53` (~2026-08-18, store
  `dp538z07…`); both `orbis-hardwared` (system) and `orbis-sessiond` (user)
  ran that revision.
- `~/.nixos/flake.lock` input `orbis-control` updated:
  `4d29ed53…` → `7db695c4…`; `nixos-rebuild build` PASS;
  owner executed `sudo nixos-rebuild switch --flake ~/.nixos#nixos`.
- Post-switch running binary verified: `/nix/store/96xcnyn6…/bin/orbis-hardwared`.

## Sandbox assertions on the effective live unit (#126)

```
systemctl show orbis-hardwared.service
  ActiveState=active
  ProtectSystem=strict
  NoNewPrivileges=yes
  ReadWritePaths=-/sys/firmware/acpi/platform_profile     ← only write surface
```

Blocked keyboard brightness path absent from the sandbox; polkit actions for
fan/gpu/panel/keyboard ship `allow_active=no`; performance/charge-limit are
`allow_active=yes`. The host deploys via the NixOS module; the standalone
`deploy-dev-hardwared.sh` flow therefore refuses to run here by design
(NixOS-owned symlink unit) — the standalone variant remains covered by the
`hardwared-lifecycle` VM check plus the unit-parity source contract.

## Battery mutation status — dynamic requery vs real asusd (#107)

- Stale revision (`4d29ed53`, boot-time preflight): `BatteryMutationStatus()`
  returned `y 2` = `TemporarilyUnavailable` although asusd owned its name —
  consistent with the startup preflight race #107 removed.
- Current revision, same live asusd: two sequential calls returned `y 0` =
  `Supported` (owner-liveness requery per call, non-activating).

## Controlled mutations (current revision, all restored)

### Performance profile

1. Manual round-trip via Hardware1:
   - initial kernel state `/sys/firmware/acpi/platform_profile` = `quiet`;
   - `SetPerformanceProfile(1)` → `y 1` (`ApplyResult::Applied`, wire echoes
     the applied profile) → kernel read-back `balanced`;
   - restore `SetPerformanceProfile(0)` → `y 0` → kernel read-back `quiet`.
2. Canonical CLI tool:

```
$ echo APPLY-TEST | orbisctl validate platform-profile --apply-test
initial state: Some(Silent)
available choices: [Silent, Balanced, Turbo]
write capability: Some(Supported)
requested state: Some(Balanced)
applied state: Some(Balanced)
read-back: Some(Balanced)
restore state: Some(Silent)
result: PASS (profile applied, verified, and restored)
```

### Battery charge limit

- Initial: configured/effective `100`, sysfs
  `charge_control_end_threshold = 100`.
- `SetChargeLimit(80)` → returned confirmed configured percent `80`; kernel
  threshold read-back `80`; Session1 evidence `state: Confirmed` with
  AsusBackend+Sysfs consensus.
- Restore `SetChargeLimit(100)` → kernel read-back `100`.

## Honest negative / degradation evidence

- Per-fan Session1 reads (`Session1.FanCurve(profile, fan)`) fail closed
  against the real daemon: stored asusd CPU curve contains temperature
  sentinel `153` → strict range decode rejects it
  («asusd FanCurves: температура вне диапазона '153' для CPU»). This is local
  `Malformed`-class evidence, not fake defaults; consequently no aggregate
  `FanCurves` capability is published on this host right now (#109 contract
  observed live).
- GPU power read stays honestly `Unavailable` (supergfxd not installed);
  product-mode Armoury snapshot decodes Hybrid with nothing queued;
  physical MUX `Integrated`; access `Unblocked`.
- Fan/Panel/Keyboard/Aura mutation statuses remain `Unsupported (1)` from the
  production composition.

## What this evidence does NOT cover

- Fan curve custom writes / factory reset (#104/#105): still hard-blocked in
  production (`DisabledFanMutationBackend` + `allow_active=no`). Live
  controlled validation requires a dedicated dev harness reaching the dormant
  backend; not executed in this session yet.
- Hosted CI execution (#106, skipped by owner decision); packaging acceptance
  beyond this host; other hardware models.
