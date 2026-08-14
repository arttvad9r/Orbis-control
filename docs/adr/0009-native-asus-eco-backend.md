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

## Host evidence: DRM release boundaries

На host доказан отдельный safe primitive для `ReleaseSecondaryDrmDevice`:
synthetic `remove` в NVIDIA DRM card-minor `uevent` освободил card FDs у PID 1,
`systemd-logind`, KWin и Xwayland live; Plasma/KWin/Xwayland при этом сохранили
работоспособность. Это не доказывает release render-node или NVIDIA core state.

Отдельный synthetic `remove` для NVIDIA render-minor `renderD129` дал только
частичный эффект: часть render/core holders и module refs уменьшилась, но
KWin сохранил render/core users; также был зафиксирован временный лог
`The main thread was hanging temporarily!`. KWin не завершился, не
перезапускался (`NRestarts=0`), coredump отсутствовал, а graphical session
осталась healthy. Поэтому render-minor notification не является принятым
live-release primitive и не должен входить в production execution sequence.

`ReleaseSecondaryDrmDevice` и release `renderD129`/NVIDIA core backend — разные
stages. После safe card release наличие render-node holders, `/dev/nvidia0`,
`/dev/nvidiactl`, `/dev/nvidia-modeset` или compositor mappings сохраняет
firmware transition fail-closed до `VerifyCompositorRelease` и последующих
`UnloadNvidiaModules`/`VerifyNvidiaModuleUnload` stages. Это не является
доказательством необходимости logout: logout пока остаётся unresolved, а не
автоматическим `RequiresLogout`.

LACT/UVM — отдельная lifecycle stage: LACT direct `/dev/nvidia-uvm` holders
сопровождались `nvidia_uvm refcount=4`; LACT teardown дал holders `0` и
refcount `0`, после чего отдельный live `modprobe -r nvidia_uvm` прошёл.
Это evidence не смешивается с KWin render/core blocker.

## Reference-derived PCI release boundary

Read-only audit `utajum/g-helper-linux` commit `b1322417...` подтверждает
последовательность `ReleaseApplicationGpuUsers` → card-level DRM remove
notification → PCI function unbind → refcount settle → holder purge → NVIDIA
module unload → firmware Eco transition. G-Helper перечисляет PCI functions
одного dGPU, unbind'ит их в reverse order, затем имеет timeout/late-completion
polling, rollback и reboot/defer fallback.

В reference не найден отдельный KWin API или принятый render-minor release
primitive. Protected compositor/system holders не kill'ятся. PCI unbind
используется как release-stage primitive, но G-Helper не доказывает перед ним
полное исчезновение compositor render/core holders и частично полагается на
hot-remove/device-removal semantics.

Для Linux PCI/NVIDIA lifecycle это не является host-specific safety proof:
PCI unbind может начаться при открытых userspace FDs, PCI core их не закрывает,
DRM unplug инвалидирует device и unregister'ит minors, но не закрывает уже
открытые file objects. NVIDIA remove/modeset lifecycle ожидает quiesced/closed
clients и может ждать usage refs. Поэтому Native Eco должен оставаться
fail-closed до `VerifyCompositorRelease` либо отдельного контролируемого
session-isolated workflow. После доказанного compositor release загруженный
module stack остаётся отдельной следующей lifecycle stage; firmware transition
не может перескочить через unload/read-back verification.

Это evidence feasibility reference implementation, а не доказательство
безопасности PCI unbind на текущем graphical host. Logout/restart compositor
остаётся возможной будущей strategy, но не product requirement и не должен
автоматически превращаться в `RequiresLogout` только из-за holders.

PCI-unbind executor в Orbis не реализуется. Production path
`Hardware1.SetGpuMode → supergfxd` и ownership boundary остаются неизменными.
