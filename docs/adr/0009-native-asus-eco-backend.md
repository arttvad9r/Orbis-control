# ADR 0009: Native ASUS live Eco preflight

- Статус: **Принято как read-only foundation** (2026-08-14)

## Решение

`dgpu_disable` firmware ABI сам по себе не требует logout или reboot. Kernel
WMI/Armoury driver только передаёт firmware-команду; он не выгружает драйверы,
не удаляет PCI-устройство и не управляет display manager.

Установленный `supergfxd 5.2.7` для `Hybrid → Integrated` использует
conservative full graphics lifecycle: WaitLogout, StopDisplayManager, release
NVIDIA/PCI state и StartDisplayManager. Это отдельный design choice, а не
доказанное требование firmware ABI.

Прямой `dgpu_disable` writer запрещён как недостаточный: без проверки DRM,
I²C, module и compositor state он может оставить GPU lifecycle в частичном
состоянии.

Native live Eco будет отдельным будущим backend/capability. Сначала Orbis
реализует только read-only preflight, typed evidence и pure release planning.
Readiness означает возможность построить безопасный план, а не немедленную
возможность записи. Native backend не
подменяет и не меняет текущий supergfxd backend.

Одновременное владение одной GPU lifecycle state со стороны supergfxd и native
backend запрещено без отдельного explicit ownership contract. Product mapping
`Eco/Standard` пока не привязывается автоматически к native backend.

Current production supergfxd path и ownership остаются неизменными. Этот ADR
не утверждает migration plan `supergfxd → native`.

## Scope

Preflight классифицирует только:

- `Ready`;
- typed `CanBecomeReady` с release requirements и unresolved verification;
- severity buckets `HardBlocker`, `ReleaseRequired`, `Informational`, `Unknown`;
- `Unsupported`;
- `Inconsistent`.

Проверяются Armoury ABI, MUX/access, PCI/NVIDIA state, `/proc` FD и mappings,
DRM/I²C users, module refcounts и read-only supergfxd coordination. Library-only
mapping и aggregate module refcount являются evidence, а не автоматическими
hard blockers. При этом загруженный NVIDIA module stack является отдельной
release lifecycle stage: zero userspace holders не доказывает unloadability,
а module unload fallible и требует read-back verification. Firmware disable не
может начинаться до доказанного release. На текущем host exact ownership
NVIDIA UVM busy reference остаётся unresolved. Compositor/device release должен
быть подтверждён до появления mutation executor. Никаких hardware writes,
process kills, service actions или driver/PCI operations.
