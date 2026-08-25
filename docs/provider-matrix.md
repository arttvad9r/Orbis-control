# Provider Matrix — Orbis Control

> Роль: **CURRENT PROVIDER STRATEGY**. Фактическая готовность и blockers — в
> [`current-state.md`](current-state.md). Dated hardware observations живут в
> `tests/fixtures/hardware/`, research/audit documents и revision-scoped evidence;
> они не превращаются автоматически в runtime support.
>
> Обновлено: 2026-08-19.

## Principles

1. Provider support определяется runtime evidence/probes, не DMI model name.
2. Read/write evidence независимы.
3. File/object presence или successful constructor не доказывают write support.
4. Unknown/Unsupported/TemporarilyUnavailable/PermissionDenied/ReadOnly не
   взаимозаменяемы.
5. Standard kernel ABI предпочтителен, когда ownership/semantics доказаны;
   asusd/supergfxd используются как typed compatibility/owner backends для тех
   concepts, которыми они реально владеют.
6. Привилегированный write получает отдельный bounded Hardware1 method; generic
   sysfs/filesystem/shell writer запрещён.
7. `Applied` требует defined confirmation/read-back. `Accepted` остаётся
   unconfirmed state.
8. Неполный/unsafe write path остаётся fail-closed.

## Current matrix

| Concept | Production read owner/path | Production write owner/path | Current write status |
|---|---|---|---|
| Performance profile | kernel `platform_profile` → Session1 | Hardware1 → polkit → fixed kernel writer → read-back | LIVE-VALIDATED historical evidence |
| Battery charge limit | UPower policy + asusd configured + kernel effective → Session1 | Hardware1 → polkit → typed asusd setter → configured/effective confirmation | LIVE-VALIDATED historical evidence; liveness/discovery hardening #107/#108 |
| GPU runtime power | supergfxd `Power()` → Session1 | none at product level | read only |
| Physical GPU MUX | ASUS Armoury/kernel evidence → Session1 | product mutation disabled | blocked |
| dGPU access policy | ASUS Armoury/kernel evidence → Session1 | product mutation disabled | blocked |
| Product GPU mode | composed concept only | no accepted production mapping | BLOCKED |
| Fan active curve | `asus_custom_fan_curve` sysfs read | none | read only |
| Fan stored profile curve | asusd `FanCurveData(profile)` → Session1 | typed asusd owner through production Hardware1; exact read-back path | **LIVE-VALIDATED FA707NV**; aggregate read evidence is model/revision scoped |
| Telemetry | dynamic read-only sysfs/hwmon/power_supply | none | read only; coverage/freshness semantics #117 |
| Panel Overdrive | typed ASUS provider/probe | Hardware1 → typed asusd setter → local read-back | backend-ready; liveness evidence #107 |
| MiniLED | typed read provider/probe | none | read only |
| Screen Auto Brightness | typed read provider/probe | none | read only |
| Keyboard backlight | typed read state/probe | Hardware1 → fixed brightness write → bounded fresh read-back | **LIVE-VALIDATED FA707NV** |
| Aura Static RGB | typed Aura read/config state | Hardware1 → asusd Static config; honest `Accepted` semantics | PARTIAL; liveness evidence #107 |
| Wayland outputs | read-only compositor output provider | none | read only |
| Diagnostics service presence | D-Bus `NameHasOwner`/activatable checks without activation | none | read only |

## Performance

Read contract:

```text
/sys/firmware/acpi/platform_profile
/sys/firmware/acpi/platform_profile_choices
→ sessiond → Session1 → session client
```

Write contract:

```text
original application caller
→ Hardware1
→ performance polkit action
→ fixed `platform_profile` writer
→ local fresh read-back
→ fresh Session1 observation
```

Unknown wire values are rejected. The Performance VM explicitly disables UPower
so the test also proves Session1 does not couple independent Performance to
Battery availability.

## Battery

Keep sources distinct:

- UPower: policy enabled/state and general battery telemetry;
- asusd: configured charge threshold owner;
- kernel power_supply: effective threshold observation.

Production domain does not invent min/max/step when the backend did not prove
them. Historical fixture ranges are not runtime constraints.

Current hardening gaps:

- mutation status must prove the asusd write owner/interface is actually
  reachable (#107);
- discovery errors must not collapse permission/transient failure into
  structural Unsupported (#108).

## GPU

Never collapse:

```text
runtime dGPU power
physical MUX
dGPU access policy
requested product mode
pending reboot/logout requirement
```

Independent read providers may use different owners. Product mode remains
blocked until a policy is proven across these concepts. A raw supergfxd enum is
not the Orbis product mode API.

## Fans

Read concepts are separate:

- **active curve** — current sysfs `asus_custom_fan_curve` view;
- **stored profile-specific curve** — asusd `FanCurveData(profile)`.

No direct sysfs fan write is permitted by Orbis architecture. The only accepted
future mutation owner is the typed asusd fan API through Hardware1, but that path
is currently disabled.

Mandatory blockers before re-enabling fan writes:

- preserve asusd `CurveData.enabled` on custom update (#104);
- make Factory Defaults restoration failure-safe (#105);
- represent CPU/GPU support without cross-inference and allow asymmetric read
  support where valid (#109);
- preserve profile-specific `enabled` evidence through Session1/UI (#116);
- executable tests/CI and controlled live validation on the exact revision.

Packaged polkit currently denies the fan write action even for active users, and
FansWindow mutation controls are disabled. Reads remain available.

## Telemetry

`SysfsTelemetryProvider` discovers sources dynamically and reads independent
metrics without inventing unsupported totals/percentages. Partial observations
are allowed.

Evidence refinement #117 is implemented: useful recent observation is distinct
from an empty/partial successful call. One optional sensor failure does not
poison all telemetry, while denial/malformed absence does not become proof of
fresh useful data; field-local gap classes remain available for diagnostics.

## Extended ASUS controls

Use [`extended-asus-controls-readiness.md`](extended-asus-controls-readiness.md)
for concept-specific readiness.

Rules:

- Panel Overdrive is not Panel HD;
- keyboard brightness is not generic Aura brightness;
- MiniLED/Screen Auto Brightness read support does not imply write support;
- AniMe/Slash/Boot sound/MCU powersave/eGPU remain separate concepts requiring
  their own evidence;
- generic firmware integer writers are forbidden.

## Capability aggregation

Registry snapshots are immutable and contain independent operation evidence.
UI/clients gate mutation from `operations.write.status`, never overall status
alone.

Periodic refresh re-queries mutation status before rebuilding. Explicit refresh
currently needs the same canonical sequence (#112).

Write-owner liveness should be probed non-mutatingly. Battery/Panel/Aura status
still needs this hardening (#107).

## Dated hardware evidence

FA707NV evidence from 2026-08-06 remains useful for:

- proving that specific paths/objects existed on that system;
- comparing known enum/value observations;
- regression fixtures and support-matrix evidence.

It does **not** prove:

- universal ASUS support;
- current revision write safety;
- cross-model ranges/defaults;
- that a write is supported merely because a file/object was present.

For a release claim, use [`verification.md`](verification.md) and
[`release-evidence-taxonomy.md`](release-evidence-taxonomy.md).
