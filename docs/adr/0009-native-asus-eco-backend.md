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
реализует только read-only preflight и typed evidence. Native backend не
подменяет и не меняет текущий supergfxd backend.

Одновременное владение одной GPU lifecycle state со стороны supergfxd и native
backend запрещено без отдельного explicit ownership contract. Product mapping
`Eco/Standard` пока не привязывается автоматически к native backend.

Current production supergfxd path и ownership остаются неизменными. Этот ADR
не утверждает migration plan `supergfxd → native`.

## Scope

Preflight классифицирует только:

- `Ready`;
- typed `Blocked` с причиной и diagnostic evidence;
- `Unsupported`;
- `Inconsistent`.

Проверяются Armoury ABI, MUX/access, PCI/NVIDIA state, `/proc` FD и mappings,
DRM/I²C users, module refcounts и read-only supergfxd coordination. Никаких
hardware writes, process kills, service actions или driver/PCI operations.
