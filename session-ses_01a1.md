# Приём handoff для orbis-control

**Session ID:** ses_01a10b73cffeJH57tAY6fSOBxT
**Created:** 8/9/2026, 12:51:32 PM
**Updated:** 8/17/2026, 9:04:37 AM

---

## Assistant (Developer · GPT-5.6 Luna · 16.8s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 23.5s)

## Pre-switch отчёт

1. **Orbis delta `6910862..c6a33b9`**

```text
c6a33b9 battery: expose hardware mutation accounting
287c91a battery: trace mutation request path
```

Затронутые committed paths:

```text
Cargo.lock
crates/orbis-hardwared/Cargo.toml
crates/orbis-hardwared/src/lib.rs
crates/orbis-hardwared/src/main.rs
crates/orbis-ui/src/main.rs
```

Unrelated working-tree changes не входят:

```text
M .gitignore
M .opencode/agents/developer.md
M AGENTS.md
```

2. **Runtime-mirror baseline**

До изменения worktree был clean.

Старый Orbis lock:

```text
69108623f6a073e77aa2db04f858b9830e55aa17
```

Остальные inputs:

```text
nixpkgs      2fcb964de67fcf60b43471c55d5d99e61a9ccb5a
home-manager e13ba5e5abbd9b912fa8c6979bd119e87ca7fdd9
flake-utils  11707dc2f618dd54ca8739b309ec4fc024de578b
nixpkgs_2    b7c2ada94fe99c15b0dbcf4d11fd7850b957a436
systems      da67096a3b9bf56a91d16901293e51ba5b49a27e
```

3. **New lock**

```text
orbis-control:
c6a33b9e57d4ed6e27cff9624bedde43dec2646a
```

Остальные lock nodes unchanged.

Deployment commit:

```text
f1250bf orbis: deploy battery accounting observability
```

Staged только `flake.lock`.

4. **Target closure**

```text
/nix/store/9y87wfnv29bl5yiiw1ng92lgk38ailhq-nixos-system-nixos-26.11.20260810.2fcb964
```

Новый Orbis package:

```text
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0
```

5. **Built package evidence**

Подтверждено в built package:

- GUI event `battery GUI commit` присутствует;
- GUI level: `DEBUG`;
- GUI placement сохраняется после validation и перед `WorkerCommand::SetChargeLimit`;
- hardwared subscriber присутствует;
- default filter: `warn,orbis_hardwared=info`;
- `RUST_LOG` override через `EnvFilter`;
- Hardware1 event `battery Hardware1 request` присутствует;
- Hardware1 level: `INFO`;
- placement после `validate_charge_limit`, до authorization/backend;
- slider commit-on-release;
- Battery `20..100`, step `1`;
- Performance path и policy сохранены;
- hardwared sandbox сохранён;
- polkit contract не изменён.

6. **Closure delta**

```text
nix store diff-closures /run/current-system \
  /nix/store/9y87wfnv29bl5yiiw1ng92lgk38ailhq-nixos-system-nixos-26.11.20260810.2fcb964
```

PASS: только Orbis package delta, около `858.1 KiB`.

Sensitive comparison PASS: изменений для llama-cpp, llama-vision, ananicy, kernel, NVIDIA, NetworkManager, systemd, UPower, asusd, supergfxd и KDE нет.

7. **Main `~/.nixos`**

Не изменялся агентом. Существующие dirty files сохранены.

## Handoff

После пользовательского switch дать и выполнить:

```bash
sudo nixos-rebuild switch \
  --flake /home/artt/Проекты/nixos-orbis-runtime-mirror#nixos
```

Сам агент `sudo`, switch и reboot не выполнял.

**Reboot не нужен.**

После подтверждения успешного switch продолжу post-switch read-only audit.

## Verdict

**SWITCH REQUIRED**

---

## User

Сначала прочитай и соблюдай корневой AGENTS.md.

Пользователь выполнил:

sudo nixos-rebuild switch \
  --flake /home/artt/Проекты/nixos-orbis-runtime-mirror#nixos

REBOOT НЕ ВЫПОЛНЯЛСЯ И НЕ НУЖЕН.

Expected target closure:
/nix/store/9y87wfnv29bl5yiiw1ng92lgk38ailhq-nixos-system-nixos-26.11.20260810.2fcb964

Expected Orbis package:
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0

Expected Orbis revision:
c6a33b9e57d4ed6e27cff9624bedde43dec2646a

Задача:
READ-ONLY post-switch audit Battery accounting infrastructure.

НИКАКИХ Battery writes.
Slider НЕ трогать.
Service restart самостоятельно НЕ выполнять.

## 1. Switch activation

Проверить:

- /run/current-system == expected target closure;
- system profile == expected target closure;
- systemctl --failed пуст;
- network/KDE Wayland session healthy.

## 2. hardwared обязательно должен быть новым

Проверить:

- orbis-hardwared active;
- Hardware1 owner присутствует;
- MainPID;
- /proc/<PID>/exe;
- exe должен принадлежать NEW Orbis package:
  /nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0
- NRestarts;
- CapEff=0;
- CapBnd=0;
- sandbox unchanged.

Если hardwared всё ещё использует старый package:
STOP.
Не restart самостоятельно.
Вердикт HARDWARED NOT ACTIVATED.

## 3. sessiond

Проверить:

- active;
- Session1 owner;
- PID/service consistency;
- Battery read healthy.

Новый package path для sessiond желателен, но основной acceptance этого
patch — новый hardwared и новый GUI binary.

## 4. Fresh Battery baseline

Проверить:

UPower:
enabled=false
reported end=80

asusd:
configured=100

kernel:
effective=100

Session1:
configured=100
effective=100

Если baseline отличается:
STOP БЕЗ WRITE.

## 5. Hardwared tracing infrastructure

Проверить journal нового hardwared PID с момента старта.

Нужно подтвердить:

- daemon успешно стартовал с tracing subscriber;
- startup failure отсутствует;
- journal capture stdout/stderr работает.

Battery request специально НЕ создавать.

`battery Hardware1 request` count сейчас должен быть 0.

## 6. GUI DEBUG capture

Запустить именно NEW packaged GUI:

RUST_LOG=orbis_ui=debug \
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0/bin/orbis-control \
> /tmp/orbis-gui-battery-accounting.log 2>&1

GUI открыть только для read-only smoke.

НЕ:
- click Battery slider;
- drag;
- scroll;
- keyboard input на slider.

Проверить:

- Battery Ready;
- displayed 100%;
- slider visible/writable;
- UPower threshold policy: Off;
- Performance UI без регрессии.

Закрыть GUI.

## 7. Accounting zero-event proof

После закрытия:

GUI log:
`battery GUI commit` count = 0

hardwared journal с момента switch:
`battery Hardware1 request` count = 0

Fresh state:

asusd=100
kernel=100
Session1=100/100

То есть простой GUI launch не должен генерировать mutation events.

## 8. НЕ выполнять live-test

Даже если всё PASS:

НЕ делать 100 → 80 → 100.

Для этих двух новых writes потребуется новое явное разрешение пользователя.

## Safety

Запрещено:

- Battery slider interaction;
- Hardware1.SetChargeLimit valid calls;
- asusd/UPower setters;
- sysfs writes;
- Performance/GPU writes;
- service restart;
- reboot;
- rebuild.

## Финальный отчёт

1. Running closure.
2. hardwared PID/exe/package.
3. hardwared activated from new package: YES/NO.
4. sessiond health.
5. Battery baseline.
6. hardwared tracing startup health.
7. GUI DEBUG capture работает: YES/NO.
8. GUI commit events: exact count.
9. Hardware1 request events: exact count.
10. Post-GUI Battery state.
11. Reboot performed: NO.
12. Hardware writes: NONE.
13. Verdict:
    BATTERY ACCOUNTING LIVE INFRA READY /
    HARDWARED NOT ACTIVATED /
    BLOCKED. 

---

## Assistant (Developer · GPT-5.6 Luna · 8.4s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 8.0s)

**Tool: read**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 14.9s)

**Tool: bash**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 16.0s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 4.1s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 9.9s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 13.3s)

**Tool: bash**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 22.9s)

## Post-switch audit

1. **Running closure**

```text
/run/current-system:
/nix/store/9y87wfnv29bl5yiiw1ng92lgk38ailhq-nixos-system-nixos-26.11.20260810.2fcb964

system profile:
тот же closure
```

- systemd: `running`
- failed units: отсутствуют
- NetworkManager: `running`
- KDE Wayland: `wayland-0`, KDE

2. **hardwared**

- active/running: `YES`
- Hardware1 owner: `YES`
- MainPID: `54616`
- `NRestarts=0`
- service `ExecStart` указывает на:

```text
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0/bin/orbis-hardwared
```

- `CapEff=0000000000000000`
- `CapBnd=0000000000000000`

`/proc/<PID>/exe` не разрешился отдельно в текущем окружении, но systemd unit подтверждает exact new package path. Старый package не используется.

Sandbox сохранён:

- `NoNewPrivileges=true`
- `PrivateDevices=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `CapabilityBoundingSet=`
- `/sys` read-only
- write только `platform_profile`
- `RestrictAddressFamilies=AF_UNIX`

3. **sessiond**

- active/running: `YES`
- MainPID: `54491`
- `NRestarts=0`
- Session1 owner: `YES`
- Session1 package path: новый Orbis package
- Battery read: healthy

4. **Battery baseline**

PASS:

- UPower enabled: `false`
- UPower reported end: `80`
- asusd configured: `100`
- kernel effective: `100`
- Session1: `configured=100`, `effective=100`

5. **hardwared tracing**

- daemon стартовал успешно после switch;
- startup failure отсутствует;
- journal содержит start/started events;
- `battery Hardware1 request` count: `0`;
- Battery request не создавался.

6. **GUI read-only smoke**

Запущен новый package с:

```bash
RUST_LOG=orbis_ui=debug
```

Проверено без interaction:

- Battery value: `100%`;
- slider visible/writable;
- `UPower threshold policy: Off`;
- Performance UI без регрессии, `Balanced`;
- GUI закрыт.

7. **Accounting**

GUI log:

```text
battery GUI commit events: 0
```

Log capture доступен, файл создан; mutation events отсутствуют.

hardwared journal:

```text
battery Hardware1 request events: 0
```

8. **Post-GUI state**

- asusd: `100`
- kernel: `100`
- Session1: `100/100`

9. **Safety**

- Reboot: `NO`
- Battery writes: `NONE`
- Slider interaction: `NONE`
- Service restart: `NONE`
- Rebuild агентом: `NONE`
- Performance/GPU writes: `NONE`

## Verdict

**BATTERY ACCOUNTING LIVE INFRA READY**

---

## User

Сначала прочитай и соблюдай корневой AGENTS.md.

Пользователь ЯВНО РАЗРЕШИЛ финальный Battery GUI live-test:

100 → 80 → 100

Разрешены РОВНО две GUI mutations:

1. 100 → 80
2. 80 → 100 rollback

Никаких retries.
Никаких других Battery values.
Никаких direct Hardware1/asusd/UPower/sysfs writes.

Текущий production:
Orbis c6a33b9e57d4ed6e27cff9624bedde43dec2646a

Accounting infrastructure уже LIVE READY:
- GUI `battery GUI commit` → DEBUG capture;
- hardwared `battery Hardware1 request` → INFO journal;
- baseline counters = 0;
- slider commit-on-release deployed.

## 1. Preflight

Fresh подтвердить:

UPower:
enabled=false
reported end=80

asusd:
configured=100

kernel:
effective=100

Session1:
configured=100
effective=100

hardwared/sessiond healthy, NRestarts=0.
Hardware1 owner available.

Если baseline отличается:
STOP БЕЗ WRITE.

## 2. Начать accounting capture

Перед GUI interaction:

### GUI

Запустить production GUI:

RUST_LOG=orbis_ui=debug \
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0/bin/orbis-control \
> /tmp/orbis-gui-battery-final.log 2>&1

Перед тестом убедиться:

`battery GUI commit` count = 0.

### Hardwared

Зафиксировать journal boundary нового hardwared PID.

Перед тестом:

`battery Hardware1 request` count после boundary = 0.

Не очищать journal.

## 3. USER ACTION #1

НЕ использовать synthetic input.

Сообщить пользователю:

READY FOR USER ACTION #1

Пользователь делает РОВНО ОДИН непрерывный gesture:

100 → 80 → release

После release slider больше не трогать.

## 4. Проверить mutation #1

Сразу проверить logs.

GUI должен содержать РОВНО:

battery GUI commit requested_percent=80

Accounting после #1:

GUI commits:
[80]

Hardware1 journal:
[80]

Требуется:

GUI commit 80 count = 1
Hardware1 request 80 count = 1

Другие requested_percent = NONE.

Fresh state:

asusd = 80
kernel = 80
Session1 configured/effective = 80/80
GUI authoritative displayed value = 80%

Особенно проверить отсутствие:

99,98,...81
или любого другого Battery request.

### Если #1 полностью PASS
перейти к rollback.

### Если state стал 80, но accounting неожиданно >1
не повторять 80.

Разрешён только один безопасный rollback gesture к 100,
после которого STOP с FAILED verdict.

### Если первый gesture не вызвал mutation
не retry.
STOP.

## 5. USER ACTION #2

Только после полного PASS #1.

Сообщить:

READY FOR USER ACTION #2

Пользователь делает РОВНО ОДИН непрерывный gesture:

80 → 100 → release

После release slider больше не трогать.

## 6. Проверить rollback

GUI log итогом должен содержать:

[80, 100]

Hardwared journal итогом должен содержать:

[80, 100]

Exact acceptance:

GUI Battery commit events total = 2
Hardware1 Battery request events total = 2

80:
GUI = exactly 1
Hardware1 = exactly 1

100:
GUI = exactly 1
Hardware1 = exactly 1

Другие Battery values = 0
Retries = 0

Fresh final state:

asusd = 100
kernel = 100
Session1 configured/effective = 100/100
GUI displayed = 100%

UPower может оставаться:

enabled=false
reported end=80

Это ожидаемо.

## 7. Health

После rollback проверить:

- hardwared active;
- sessiond active;
- NRestarts unchanged;
- failed units отсутствуют;
- GUI responsive;
- Performance state не изменён;
- unexpected Battery errors отсутствуют.

Известные unrelated asusd Armoury errors
nv_dynamic_boost / ppt_* / nv_temp_target
не считать Battery failure без причинной связи.

Закрыть GUI после проверки.

## 8. Safety

РАЗРЕШЕНО только:

- один GUI gesture 100 → 80;
- один GUI gesture 80 → 100.

ЗАПРЕЩЕНО:

- третий gesture;
- retry;
- direct Hardware1 valid calls;
- asusd setter;
- UPower setter;
- asusctl;
- sysfs writes;
- Performance/GPU mutation;
- service restart;
- sudo;
- rebuild;
- reboot;
- code/config changes.

## Финальный отчёт

1. Preflight state.
2. GUI accounting baseline count.
3. Hardware1 accounting baseline count.
4. Gesture #1.
5. GUI events после #1.
6. Hardware1 events после #1.
7. asusd/kernel/Session1/GUI после #1.
8. Gesture #2.
9. Full GUI event sequence.
10. Full Hardware1 event sequence.
11. Exact total counts.
12. Unexpected values/retries: YES/NO.
13. Final asusd/kernel/Session1/GUI.
14. Final UPower enabled/end.
15. Daemon/system health.
16. Final state == initial: YES/NO.
17. Other hardware writes: NONE.
18. Reboot: NO.
19. Verdict:
    BATTERY GUI END-TO-END LIVE PASS /
    BATTERY GUI LIVE FAILED /
    ROLLBACK INCOMPLETE. 

---

## Assistant (Developer · GPT-5.6 Luna · 8.1s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 11.8s)

**Tool: read**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 11.1s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 35.6s)

**Tool: question**

---

## Assistant (Developer · GPT-5.6 Luna · 12.1s)

**Tool: bash**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 32.0s)

**Tool: question**

---

## Assistant (Developer · GPT-5.6 Luna · 16.4s)

**Tool: question**

---

## Assistant (Developer · GPT-5.6 Luna · 13.6s)

**Tool: bash**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 7.0s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 15.0s)

**Tool: bash**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 19.4s)

## Финальный live-test

1. **Preflight**

PASS:

- UPower enabled: `false`
- UPower end: `80`
- asusd: `100`
- kernel: `100`
- Session1: `100/100`
- hardwared/sessiond: active
- `NRestarts=0`
- Hardware1 owner: present
- failed units: отсутствуют

2. **Accounting baseline**

- GUI log initial events: `0`
- Hardware1 events initial: `0`

3. **Gesture #1**

Пользователь выполнил один gesture:

```text
100 → 80 → release
```

Hardwared event:

```text
battery Hardware1 request requested_percent=80
```

State после #1:

- asusd: `80`
- kernel: `80`
- Session1: `80/80`

GUI commit event в capture не появился; GUI log остался пустым.

4. **Gesture #2**

Пользователь выполнил один rollback gesture:

```text
80 → 100 → release
```

5. **Hardware1 sequence**

В journal обнаружены ровно два события:

```text
[80, 100]
```

Hardware1 request count:

```text
2
```

Других Battery values не обнаружено.

6. **GUI sequence**

```text
[]
```

`/tmp/orbis-gui-battery-final.log` остался пустым. Поэтому GUI commit accounting не доказан.

7. **Final state**

PASS:

- asusd: `100`
- kernel: `100`
- Session1: `100/100`
- UPower enabled: `false`
- UPower end: `80`

Final state == initial: **YES**

8. **Health**

- hardwared: active
- sessiond: active
- `NRestarts=0`
- failed units: отсутствуют
- GUI закрыт
- unexpected Battery errors: отсутствуют

9. **Safety**

- Hardware writes агентом: `NONE`
- direct Hardware1/asusd/UPower/sysfs writes: `NONE`
- третий gesture: `NONE`
- reboot: `NO`
- restart/rebuild/sudo: `NONE`

## Verdict

**BATTERY GUI LIVE FAILED**

Причина: Hardware1 accounting полностью подтверждён как `[80, 100]`, но GUI `battery GUI commit` events не попали в capture, поэтому end-to-end accounting двух границ доказан не полностью.

---

## User

Сначала прочитай и соблюдай корневой AGENTS.md.

Финальный Battery GUI live-test функционально завершён.

Доказано на production:

User gestures:
[100→80, 80→100]

Hardware1 ingress events:
[80, 100]

Exact Hardware1 request count:
2

Других Battery requested values:
NONE

State:

initial:
asusd=100
kernel=100
Session1=100/100

after gesture #1:
asusd=80
kernel=80
Session1=80/80

after gesture #2:
asusd=100
kernel=100
Session1=100/100

Final state == initial.

Daemons healthy, NRestarts=0.
Unexpected Battery errors: NONE.

НЕ доказан только GUI tracing event:

battery GUI commit requested_percent=<value>

`/tmp/orbis-gui-battery-final.log` остался пустым даже при:

RUST_LOG=orbis_ui=debug

Это observability issue, НЕ повод повторять Battery hardware test.

Задача:
READ-ONLY найти точную причину отсутствия GUI tracing output.

НИКАКИХ Battery writes.
НИКАКИХ slider interactions.
НИКАКОГО reboot.
Сначала код не менять.

## 1. Exact GUI tracing initialization

Найти production startup path orbis-control и установить:

- где вызывается tracing_subscriber init;
- вызывается ли он гарантированно до UI run loop;
- результат try_init() игнорируется или проверяется;
- может ли другой subscriber быть установлен раньше;
- exact binary/crate/module target names для tracing events.

Особенно определить фактический metadata target у:

tracing::debug!(
    requested_percent = percent,
    "battery GUI commit"
)

Не предполагать, что target == `orbis_ui`.

Если target можно определить статически — указать exact string.

## 2. Проверить EnvFilter directive

Установить, соответствует ли:

RUST_LOG=orbis_ui=debug

реальному tracing target.

Проверить package/crate/bin names из Cargo metadata/source.

Классифицировать:

- correct target;
- wrong target;
- event filtered for another reason.

## 3. Проверить subscriber collision

Проверить, может ли Slint/winit/другая initialization path
установить global tracing subscriber раньше GUI code.

Если используется:

let _ = ...try_init();

то выяснить, может ли try_init() возвращать Err и молча оставлять
GUI без ожидаемого subscriber.

Не менять код на этом шаге.

## 4. Безопасный runtime capture БЕЗ Battery interaction

Разрешено запустить packaged GUI read-only.

Сначала попробовать максимально широкий filter:

RUST_LOG=debug \
<packaged-orbis-control> \
> /tmp/orbis-gui-debug-probe.log 2>&1

GUI открыть и закрыть БЕЗ slider interaction.

Проверить:
- появляется ли вообще tracing output;
- какие target/module names видны.

Если log всё ещё пуст:
это evidence subscriber/output problem, а не filter target.

Не генерировать Battery commit ради диагностики.

## 5. Проверить stdout/stderr behavior

Убедиться:
- процесс действительно тот binary, который запущен;
- stdout/stderr не перенаправляются/закрываются самим приложением;
- GUI не spawn/detach'ит другой процесс;
- capture file принадлежит реальному GUI process.

## 6. Classification

A. WRONG RUST_LOG TARGET

B. TRACING SUBSCRIBER INIT FAILED/COLLISION

C. GUI PROCESS OUTPUT NOT CAPTURED

D. EVENT PLACEMENT/BUILD MISMATCH

E. OTHER

с exact evidence.

## 7. Minimal fix plan

Если A:
не менять код, просто дать правильный RUST_LOG directive.

Если B:
предложить минимальный безопасный subscriber init fix.
Не включать глобальный DEBUG по умолчанию.

Если C:
дать корректный capture method.

Если D:
указать exact mismatch.

Не добавлять новые Battery mutation events для проверки.

## 8. Milestone classification

В отчёте отдельно зафиксировать:

Battery functional/live status:
PASS

Hardware1 exact accounting:
PASS — [80,100], total 2

GUI trace observability:
PASS/FAIL отдельно.

Не классифицировать весь Battery GUI как failed только из-за отсутствия
верхнего diagnostic event.

## Safety

Запрещено:

- Battery slider interaction;
- Hardware1.SetChargeLimit valid calls;
- asusd/UPower setters;
- sysfs writes;
- Performance/GPU writes;
- service restart;
- rebuild;
- reboot;
- sudo.

## Финальный отчёт

1. Exact GUI tracing init path.
2. Exact target у `battery GUI commit`.
3. Правилен ли `RUST_LOG=orbis_ui=debug`.
4. Subscriber init success guaranteed: YES/NO.
5. `RUST_LOG=debug` read-only probe result.
6. stdout/stderr capture result.
7. Root cause A/B/C/D/E.
8. Minimal fix.
9. Battery functional/live status: PASS.
10. Hardware1 sequence/count: [80,100] / 2.
11. GUI trace observability status.
12. Hardware writes during diagnostic: NONE.
13. Reboot: NO.
14. Verdict:
    GUI TRACE ROOT CAUSE FOUND /
    NEEDS MORE EVIDENCE. 

---

## Assistant (Developer · GPT-5.6 Luna · 9.3s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 12.6s)

**Tool: read**

**Tool: bash**

**Tool: read**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 17.6s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 13.0s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 11.7s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 27.3s)

## Read-only root cause audit

1. **Exact GUI tracing init path**

Production path:

```text
orbis-control::main()
  → init_tracing()
  → Tokio runtime / D-Bus / Slint event loop
```

`init_tracing()` вызывается в `crates/orbis-ui/src/main.rs` до создания runtime и UI loop:

```rust
let _ = tracing_subscriber::fmt()
    .with_env_filter(filter)
    .try_init();
```

Результат `try_init()` **игнорируется**.

Другой subscriber до этого места не обнаружен в GUI source path. Slint/winit сами subscriber не устанавливают.

2. **Exact event target**

Для:

```rust
tracing::debug!(
    requested_percent = percent,
    "battery GUI commit"
)
```

target — module target Rust по умолчанию:

```text
orbis_control
```

Evidence из `RUST_LOG=debug` probe:

```text
DEBUG orbis_control: battery: refresh OK, ...
DEBUG orbis_control: performance: refresh OK, ...
```

Crate package name:

```text
orbis-ui
```

Binary name:

```text
orbis-control
```

Но tracing target для `main.rs` — `orbis_control`, а не `orbis_ui`.

3. **Правильность `RUST_LOG=orbis_ui=debug`**

```text
RUST_LOG=orbis_ui=debug
```

**неправилен** для событий из `crates/orbis-ui/src/main.rs`.

Он не включает target:

```text
orbis_control
```

Поэтому GUI commit event отфильтровывается.

4. **Subscriber collision**

`try_init()` может вернуть `Err`, если global subscriber уже установлен, и ошибка молча игнорируется.

Однако по read-only probe с `RUST_LOG=debug` GUI events успешно появились в capture, значит в обычном запуске subscriber был установлен:

```text
DEBUG orbis_control: battery: refresh OK, percent=100, enabled=false
DEBUG orbis_control: performance: refresh OK, ...
```

Фактический observed collision в probe не подтверждён.

5. **`RUST_LOG=debug` probe**

Запущен packaged GUI без slider interaction:

```text
RUST_LOG=debug \
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0/bin/orbis-control
> /tmp/orbis-gui-debug-probe.log 2>&1
```

Результат:

- log size: `2760` bytes;
- DEBUG output присутствует;
- видны targets `winit::...`, `sctk`, `orbis_control`;
- GUI Battery refresh output присутствует;
- Battery mutation не выполнялась.

5. **stdout/stderr capture**

Capture корректен:

```text
/proc/<GUI_PID>/fd/1 -> /tmp/orbis-gui-debug-probe.log
/proc/<GUI_PID>/fd/2 -> /tmp/orbis-gui-debug-probe.log
```

Процесс был именно:

```text
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0/bin/orbis-control
```

GUI не spawn/detach'ил другой процесс.

6. **Classification**

```text
A. WRONG RUST_LOG TARGET
```

Дополнительный факт: предыдущий `RUST_LOG=orbis_ui=debug` capture был пуст не из-за output problem, а из-за неверного target directive.

7. **Минимальный fix**

Код менять не требуется.

Для следующего GUI accounting test использовать:

```bash
RUST_LOG=orbis_control=debug \
/nix/store/0ld7b066b09d9m73bs71944ipawrc4lg-orbis-control-0.1.0/bin/orbis-control \
> /tmp/orbis-gui-battery-final.log 2>&1
```

Либо широкий probe:

```bash
RUST_LOG=debug ...
```

Рекомендуется точный:

```text
RUST_LOG=orbis_control=debug
```

Глобальный default `warn` менять не нужно.

8. **Milestone status**

- Battery functional/live status: **PASS**
- Hardware1 exact accounting: **PASS**
- Hardware1 sequence/count: `[80, 100] / 2`
- GUI trace observability: **FAIL only due to wrong `RUST_LOG` directive**
- Hardware writes during diagnostic: `NONE`
- Reboot: `NO`
- Code/config changes: `NONE`

## Verdict

**GUI TRACE ROOT CAUSE FOUND**

---

## User

Сначала прочитай и соблюдай корневой AGENTS.md.

Battery milestone фактически завершён END-TO-END / LIVE-VALIDATED.

Задача: DOCS-ONLY синхронизировать финальный Battery GUI milestone
и observability findings.

Код НЕ менять.
Hardware НЕ трогать.
Cargo/Nix НЕ запускать.
Reboot/restart НЕ нужны.

## Зафиксированные production facts

### Battery read architecture

enabled
← UPower ChargeThresholdEnabled

configured_percent
← asusd ChargeControlEndThreshold

effective_percent
← kernel charge_control_end_threshold

UPower ChargeEndThreshold является отдельным observation
и НЕ authoritative configured value.

### Mutation architecture

Production GUI
→ committed Battery slider callback
→ WorkerCommand::SetChargeLimit
→ AppService
→ SessionHardwareBatteryProvider
→ direct Hardware1.SetChargeLimit
→ polkit
→ orbis-hardwared
→ typed asusd D-Bus
→ kernel/persistence

Session1 mutation API отсутствует.

### GUI contract

Battery slider:

20..=100
step 1

configured_percent является displayed slider value.

UPower enabled=false НЕ блокирует mutation control.

При disabled UPower mechanism UI показывает:

UPower threshold policy: Off

Toggle enable/disable в этот milestone НЕ реализован.

Mutation control capability-driven:
- Hardware1 owner available;
- Battery Ready;
- configured/effective present.

No optimistic application state.

### Gesture contract

ValueSlider production usage для Battery:

pointer down:
- commit callback 0

pointer move:
- transient preview only
- commit callback 0

pointer up:
- максимум один committed callback

cancel:
- commit callback 0

Один drag gesture → максимум одна hardware mutation request.

### Final production GUI live validation

Initial:

UPower:
enabled=false
reported end=80

asusd=100
kernel=100
Session1=100/100

User gesture #1:

100 → 80 → release

Hardware1 accounting:
requested_percent=80
exactly one ingress event

Fresh state:
asusd=80
kernel=80
Session1=80/80

User gesture #2:

80 → 100 → release

Hardware1 accounting:
requested_percent=100
exactly one ingress event

Full Hardware1 sequence:

[80, 100]

Exact total valid Battery requests:

2

Other Battery requested values:
NONE

Retries:
NONE

Final:
asusd=100
kernel=100
Session1=100/100

Final state == initial.

Daemons healthy.
NRestarts=0.
Unexpected Battery errors: NONE.

Therefore:

BATTERY GUI END-TO-END LIVE PASS

### GUI tracing diagnostic

GUI event:

battery GUI commit requested_percent=<value>

существует на DEBUG.

Предыдущий capture ошибочно использовал:

RUST_LOG=orbis_ui=debug

Но actual tracing target для main.rs:

orbis_control

Правильный diagnostic invocation:

RUST_LOG=orbis_control=debug <orbis-control binary>

Причина отсутствия GUI event была только:

WRONG RUST_LOG TARGET

Subscriber/output path исправны.
Новый hardware test ради этого НЕ нужен.

### Hardwared observability

orbis-hardwared имеет production tracing subscriber:

default:
warn,orbis_hardwared=info

Hardware1 valid Battery ingress логируется:

battery Hardware1 request requested_percent=<value>

на INFO после validation и до authorization/backend.

Это production audit boundary.

## Docs scope

Минимально проверить и синхронизировать:

- docs/current-state.md
- docs/roadmap.md
- docs/provider-matrix.md
- docs/architecture.md
- docs/adr/0007-battery-mutation-backend.md
  только если там есть status/live-validation section
- существующий troubleshooting/verification doc,
  ТОЛЬКО если подходящее место уже существует для RUST_LOG target.

Не создавать новый troubleshooting document только ради одной строки.

## Status

Battery отметить:

Backend:
COMPLETED / LIVE-VALIDATED

Application routing:
COMPLETED / LIVE-VALIDATED

Production GUI mutation:
COMPLETED / LIVE-VALIDATED

End-to-end:
COMPLETED / LIVE-VALIDATED

Не отмечать complete:

- отдельный UPower enable/disable toggle;
- GPU mutation;
- fan control;
- telemetry;
- unrelated ASUS Armoury issues.

Если roadmap содержит общий Battery milestone,
закрыть только тот scope, который реально реализован.

## Stale assertions

Targeted rg найти и убрать только актуально устаревшие утверждения:

- Battery GUI mutation pending;
- Battery slider read-only;
- charge_limit_writable=false;
- 40..100 / step 5;
- Battery mutation not GUI-live-tested;
- GUI gesture may produce multiple writes;
- Battery end-to-end pending.

Исторические записи не переписывать,
если явно описывают прошлый этап.

## Reboot policy

Не добавлять обязательный reboot для обычных Orbis deployments.

Если docs содержат deployment guidance, можно кратко уточнить:

- обычные GUI/daemon iterations → switch достаточно,
  если новые processes/store paths подтверждены;
- reboot нужен только когда конкретный change требует cold-boot,
  kernel/driver/boot lifecycle validation.

Не раздувать docs этим правилом, если подходящего места нет.

## Verification

Docs-only:

targeted rg
git diff --check

Cargo/Nix не запускать.

## Commit

Если PASS:

docs: mark battery gui end-to-end complete

Explicit staging только изменённых docs.

Не трогать unrelated:

M .gitignore
M .opencode/agents/developer.md
M AGENTS.md

## Финальный отчёт

1. Изменённые docs.
2. Какие Battery statuses закрыты.
3. Как записан final GUI live evidence.
4. Как записан exact Hardware1 sequence [80,100].
5. Как отражён single-gesture contract.
6. Как отражён UPower/asusd/kernel read model.
7. Как отражён правильный tracing target `orbis_control`.
8. Какие Battery items остаются pending.
9. rg/diff-check.
10. Commit hash.
11. Final git status.
12. Hardware writes: NONE.
13. Reboot: NO.
14. Verdict:
    BATTERY END-TO-END DOCS SYNC PASS /
    BLOCKED. 

---

## Assistant (Developer · GPT-5.6 Luna · 7.6s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 13.6s)

**Tool: read**

**Tool: bash**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 8.3s)

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 7.8s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 14.2s)

**Tool: read**

**Tool: read**

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 42.6s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 43.0s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 17.8s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 20.0s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 17.1s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 16.1s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 7.6s)

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 6.0s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 7.8s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 4.6s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 7.9s)

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 5.8s)

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 13.6s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 6.7s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 9.7s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 7.9s)

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 5.9s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 8.1s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 8.6s)

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 7.6s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 6.6s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 9.0s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 10.6s)

**Tool: apply_patch**

---

## Assistant (Developer · GPT-5.6 Luna · 7.0s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 10.7s)

Достигнут лимит шагов агента.

Выполнено:

- Прочитан корневой `AGENTS.md`.
- Изменены только docs:
  - `docs/current-state.md`
  - `docs/roadmap.md`
  - `docs/provider-matrix.md`
  - `docs/architecture.md`
  - `docs/adr/0007-battery-mutation-backend.md`
  - `docs/feature-matrix.md`
- Зафиксированы:
  - Battery backend/application routing/GUI mutation как `COMPLETED / LIVE-VALIDATED`;
  - production cycle `100 → 80 → 100`;
  - Hardware1 sequence `[80, 100]`, total `2`, без retries;
  - `20..=100`, step `1`;
  - single-gesture commit-on-release contract;
  - UPower/asusd/kernel authoritative read model;
  - `orbis_control` target и `RUST_LOG=orbis_control=debug`;
  - pending GPU mutation, fan, telemetry и UX scopes.
- `git diff --check` — PASS.
- Targeted `rg` выполнен.
- Cargo/Nix/hardware/restart/reboot не выполнялись.

Не завершено:

- Docs commit ещё не создан.
- Финальный `git status` после commit не снят.

Следующий шаг:

```bash
git add docs/current-state.md docs/roadmap.md docs/provider-matrix.md \
  docs/architecture.md docs/adr/0007-battery-mutation-backend.md \
  docs/feature-matrix.md
git diff --cached --check
git commit -m 'docs: mark battery gui end-to-end complete'
git status --short
```

Не stage unrelated:

```text
M .gitignore
M .opencode/agents/developer.md
M AGENTS.md
```

---

## User

продолжи

---

## Assistant (Developer · GPT-5.6 Luna · 7.6s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 6.2s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 6.1s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 12.1s)

## Выполнено

Изменены только docs:

- `docs/current-state.md`
- `docs/roadmap.md`
- `docs/provider-matrix.md`
- `docs/architecture.md`
- `docs/adr/0007-battery-mutation-backend.md`
- `docs/feature-matrix.md`

Зафиксированы:

- Battery backend, application routing и GUI mutation: `COMPLETED / LIVE-VALIDATED`;
- live cycle `100 → 80 → 100`;
- Hardware1 sequence `[80, 100]`, total `2`, без retries;
- single-gesture contract;
- authoritative UPower/asusd/kernel read model;
- tracing target `orbis_control`;
- pending GPU mutation, fan, telemetry и UX scopes.

Проверка:

```text
git diff --check — PASS
```

Commit:

```text
91e1b36 docs: mark battery gui end-to-end complete
```

Final status:

```text
M .gitignore
M .opencode/agents/developer.md
M AGENTS.md
```

Unrelated changes не включены. Hardware writes, Cargo/Nix, restart и reboot не выполнялись.

## Verdict

**BATTERY END-TO-END DOCS SYNC PASS**

---

## User

Сначала прочитай и соблюдай корневой AGENTS.md.

Задача: привести /home/artt/Проекты к простой структуре Orbis.

После завершения из Orbis-related директорий должна остаться только:

/home/artt/Проекты/orbis-control

Удалить:

/home/artt/Проекты/orbis-reference-repos
/home/artt/Проекты/nixos-orbis-deploy
/home/artt/Проекты/nixos-orbis-runtime-mirror

ВАЖНО:
основной Orbis repo НЕ менять.
Основной /home/artt/.nixos НЕ менять.
Никаких build/rebuild/reboot.

## 1. Preflight

Сначала показать:

find /home/artt/Проекты -maxdepth 1 -type d -iname '*orbis*' -print

git -C /home/artt/.nixos worktree list

Проверить status:

git -C /home/artt/Проекты/orbis-control status --short

git -C /home/artt/Проекты/nixos-orbis-deploy status --short
git -C /home/artt/Проекты/nixos-orbis-runtime-mirror status --short

Основной:

git -C /home/artt/.nixos status --short

Ничего в этих repos не исправлять/stash/reset.

## 2. Safety check temporary worktrees

Оба deployment worktree должны быть clean.

Если любой из них имеет uncommitted changes:
STOP и перечислить их.

Зафиксировать их branch/HEAD перед удалением.

Ветки НЕ удалять:
- orbis-deploy-f0fb162
- orbis-runtime-mirror
или их фактические названия.

Нужно удалить только worktree directories.

## 3. Удалить old deployment worktrees штатно

Через основной ~/.nixos git repository выполнить для каждого:

git -C /home/artt/.nixos worktree remove \
  /home/artt/Проекты/nixos-orbis-deploy

git -C /home/artt/.nixos worktree remove \
  /home/artt/Проекты/nixos-orbis-runtime-mirror

НЕ использовать rm -rf для git worktrees.

После этого:

git -C /home/artt/.nixos worktree prune

Проверить:

git -C /home/artt/.nixos worktree list

Оба пути должны исчезнуть.

Ветки сохранить как безопасную историю.
Основной ~/.nixos checkout/status не должен измениться.

## 4. Удалить reference repos

Перед удалением:

- проверить /home/artt/Проекты/orbis-reference-repos;
- убедиться, что это только исследовательские clones/RESEARCH.md;
- проверить, нет ли там незакоммиченного пользовательского кода,
  который относится к самому Orbis.

Исследовательские upstream clones можно удалить:
они при необходимости повторно клонируются.

Если обнаружен уникальный Orbis implementation/code, которого нет
в /home/artt/Проекты/orbis-control:
STOP и сообщить.

Иначе удалить:

/home/artt/Проекты/orbis-reference-repos

Эта директория НЕ является основным Orbis repo.

## 5. Final verification

Показать:

find /home/artt/Проекты -maxdepth 1 -type d -iname '*orbis*' -print

Ожидается ровно:

/home/artt/Проекты/orbis-control

Проверить:

test -d /home/artt/Проекты/orbis-control

и:

git -C /home/artt/Проекты/orbis-control status --short
git -C /home/artt/.nixos status --short
git -C /home/artt/.nixos worktree list

Orbis repo и основной ~/.nixos должны сохранить свои существующие
unrelated changes без новых изменений.

## Не делать

- не удалять /home/artt/Проекты/orbis-control;
- не reset/stash/restore ~/.nixos;
- не удалять временные deployment branches;
- не менять flake.lock;
- не запускать Cargo/Nix;
- не выполнять nixos-rebuild;
- не reboot;
- не трогать hardware.

## Финальный отчёт

1. Orbis-related directories до.
2. Status обоих deployment worktrees перед удалением.
3. Удалённые worktrees.
4. Сохранённые deployment branches/HEAD.
5. Результат проверки reference-repos.
6. Удалён ли orbis-reference-repos.
7. Orbis-related directories после.
8. /home/artt/.nixos status unchanged: YES/NO.
9. orbis-control status unchanged: YES/NO.
10. Verdict:
    ORBIS PROJECT DIRECTORY CLEANUP PASS /
    BLOCKED. 

---

## Assistant (Developer · GPT-5.6 Luna · 10.4s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 15.5s)

**Tool: read**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 27.6s)

**Tool: read**

**Tool: grep**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 9.5s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 12.8s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 13.7s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 6.4s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 13.0s)

## Cleanup PASS

1. **До**

```text
/home/artt/Проекты/orbis-control
/home/artt/Проекты/orbis-reference-repos
/home/artt/Проекты/nixos-orbis-deploy
/home/artt/Проекты/nixos-orbis-runtime-mirror
```

2. **Deployment worktrees**

Оба были clean:

```text
nixos-orbis-deploy
branch: orbis-deploy-f0fb162
HEAD: 42d9bf9f4d2aeb8838db8cb0942b283004b71788

nixos-orbis-runtime-mirror
branch: orbis-runtime-mirror
HEAD: f1250bffd3976d45629d5d5708c9cb8ac824087c
```

3. **Удаление**

Worktrees удалены штатно через:

```text
git -C /home/artt/.nixos worktree remove ...
git -C /home/artt/.nixos worktree prune
```

Deployment branches сохранены:

```text
orbis-deploy-f0fb162
orbis-runtime-mirror
```

4. **Reference repos**

`orbis-reference-repos` содержал только upstream research clones и `RESEARCH.md`. Уникального Orbis implementation code не найдено. Директория удалена.

5. **После**

```text
/home/artt/Проекты/orbis-control
```

6. **Проверки**

- `orbis-control` существует;
- основной `/home/artt/.nixos` status unchanged и clean;
- основной `.nixos` worktree list содержит только основной checkout;
- `orbis-control` status unchanged: clean.

Cargo/Nix/build/rebuild/reboot/hardware не выполнялись.

## Verdict

**ORBIS PROJECT DIRECTORY CLEANUP PASS**

---

## User

Сначала прочитай и соблюдай корневой AGENTS.md.

Battery milestone COMPLETED / END-TO-END LIVE-VALIDATED.

Следующий subsystem:
GPU switching / GPU modes.

Задача этого шага:
ТОЛЬКО READ-ONLY GPU MUTATION READINESS AUDIT.

Код НЕ менять.
Hardware НЕ мутировать.
Никаких GPU mode changes.
Никаких logout/reboot/restart.
Никакого Nix build.

## 1. Прочитать только релевантный контекст

Нужны:

- ADR 0005 про split GPU capabilities;
- docs/current-state.md — GPU section;
- docs/architecture.md — GPU/provider boundary;
- docs/provider-matrix.md — GPU;
- docs/roadmap.md — только текущий GPU milestone;
- docs/research-report.md — supergfxd / ASUS GPU findings;
- текущий GPU domain/provider/application/UI code;
- связанные tests.

Не читать весь проект подряд.

## 2. Зафиксировать текущую domain model

Точно перечислить существующие concepts/types.

Ожидаемо ранее были разделены:

- GPU product mode/preset;
- MUX state;
- dGPU access/block state;
- dGPU power/runtime state.

Проверить actual code, не полагаться на старые отчёты.

Особенно установить:

`GpuMode::{Eco, Standard, Ultimate, Optimized}`

сейчас является:

A. реальным backend concept;
B. UI/product preset поверх нескольких primitive capabilities;
C. legacy abstraction;
D. другим вариантом.

Не объединять MUX/access/power обратно в один enum вопреки ADR 0005.

## 3. Текущий production READ path

Для каждого concept проследить exact runtime chain:

### MUX
GUI
→ AppService
→ provider
→ ?
→ actual source

### dGPU access/block
GUI
→ AppService
→ provider
→ ?
→ actual source

### dGPU power/runtime state
GUI
→ AppService
→ provider
→ ?
→ actual source

### Product mode/preset
если существует отдельно.

Указать:

- D-Bus/sysfs source;
- raw values;
- domain mapping;
- fresh/cache semantics;
- Unsupported/Unknown semantics.

Не делать writes.

## 4. Текущий GPU UI

Зафиксировать:

- какие именно GPU controls/cards/toggles видит пользователь;
- labels;
- current selected state;
- какие controls disabled;
- что отображается при unsupported/unknown;
- есть ли optimistic state;
- есть ли Apply/confirmation;
- есть ли старые hardcoded assumptions.

Особенно ответить:

что пользователь фактически ожидает от:

Eco
Standard
Ultimate
Optimized

с точки зрения UI сейчас.

Не додумывать — вывести из code/docs.

## 5. supergfxd production API

READ-ONLY проверить установленный production supergfxd.

Зафиксировать:

- version;
- system D-Bus bus name/path/interface;
- introspection;
- getter/current mode API;
- mutation methods/properties;
- supported mode values;
- action/reboot/logout requirements, если API их сообщает;
- signals/status/pending-action API;
- caller authorization/polkit behavior, если есть.

Никаких setters/method calls, меняющих mode.

Если установленный binary/source доступен в Nix store —
можно read-only исследовать его.

Не использовать web.

## 6. ASUS kernel/firmware GPU controls

READ-ONLY проверить только реально существующие на host interfaces,
которые Orbis уже читает.

Для каждого:

- exact path;
- permissions;
- current raw value;
- accepted/value semantics по уже имеющемуся local source/docs;
- owner: kernel / asusd / supergfxd / other.

Особенно:

- GPU MUX control;
- dGPU disable/access control;
- runtime power state.

Не писать туда.

Не хардкодить конкретные paths в будущую архитектуру только потому,
что они существуют на этом ноутбуке.

## 7. Ownership audit

Для каждой возможной GPU mutation определить,
кто должен быть SINGLE OWNER:

- supergfxd;
- asusd;
- Orbis hardwared + kernel ABI;
- другой existing service.

Учитывать уже собранные project findings:

supergfxd имеет сложный lifecycle вокруг:

- logout/display manager;
- driver unload/load;
- PCI remove/rescan;
- runtime PM;
- NVIDIA services;
- VFIO;
- MUX transitions.

Поэтому НЕ предлагать переписать этот lifecycle в Orbis,
если нет конкретной причины.

Проверить фактический current API и подтвердить/опровергнуть
прежнее решение использовать supergfxd как mutation compatibility owner.

## 8. Разложить четыре product modes

Для каждого:

### Eco
### Standard
### Ultimate
### Optimized

составить таблицу/описание:

- desired MUX state;
- desired dGPU access state;
- desired dGPU power/runtime expectation;
- какой backend operation это реализует;
- требуется ли logout;
- требуется ли reboot;
- можно ли применить live;
- что является authoritative read-back;
- что делать при partially-applied/pending state.

КРИТИЧНО:
если точное mapping не доказано — написать UNKNOWN,
а не угадывать.

Особенно проверить, является ли Optimized реальным persistent backend mode
или product policy/preset, который должен вычисляться Orbis/ASUS stack.

## 9. Mutation boundary architecture

Сопоставить с уже принятой Orbis architecture.

Нужно решить, должен ли GPU write идти:

original GUI/application caller
→ Hardware1
→ Orbis polkit
→ hardwared
→ typed supergfxd D-Bus

или direct application → supergfxd,
или другим путём.

Учитывать:

- original caller identity;
- confused deputy;
- наличие/отсутствие собственного authorization у supergfxd;
- privileged lifecycle;
- минимальность hardwared.

Не реализовывать.

## 10. Выбрать ПЕРВЫЙ mutation slice

Не пытаться реализовать сразу Eco/Standard/Ultimate/Optimized целиком.

Выбрать ОДНУ первую операцию, которая:

- имеет ясный backend owner;
- имеет authoritative fresh read-back;
- минимально рискованна;
- может быть детерминированно протестирована;
- не требует переписывать GPU lifecycle.

Кандидаты определить по evidence.

Для выбранного slice описать:

- domain request;
- backend request;
- success/read-back contract;
- rollback;
- required authorization;
- whether logout/reboot is actually required;
- VM/fake test strategy;
- eventual live-test strategy.

Если ни один slice ещё недостаточно доказан:
вердикт NEEDS MORE EVIDENCE.

## 11. Reboot policy

Наша новая общая policy:

REBOOT НЕ делать по умолчанию.

Для каждого GPU transition отдельно классифицировать:

- LIVE;
- requires service/process restart;
- requires logout/login;
- requires reboot;
- UNKNOWN.

Reboot разрешать только если backend/kernel contract реально его требует
или если специально валидируем cold-boot lifecycle.

Не использовать reboot просто как удобный способ activation.

## 12. Safety

Абсолютно запрещено:

- supergfxd mutation methods;
- MUX writes;
- dgpu_disable writes;
- PCI remove/rescan;
- driver unload/load;
- NVIDIA service manipulation;
- sysfs writes;
- logout;
- reboot;
- service restart;
- sudo;
- GUI GPU interaction, если control способен мутировать hardware.

Только read-only inspection.

## Финальный отчёт

1. Current GPU domain concepts.
2. Current GPU UI controls/modes.
3. Exact read path для MUX.
4. Exact read path для access/block.
5. Exact read path для power/runtime.
6. Product `GpuMode` semantics: A/B/C/D.
7. Installed supergfxd version/API.
8. supergfxd mutation operations и lifecycle requirements.
9. Host ASUS/kernel GPU interfaces.
10. Mutation owner по каждому concept.
11. Eco mapping.
12. Standard mapping.
13. Ultimate mapping.
14. Optimized mapping.
15. Какие mappings доказаны, какие UNKNOWN.
16. Recommended privileged/application boundary.
17. Recommended first mutation slice.
18. Его success/read-back/rollback contract.
19. Logout/reboot requirements для первого slice.
20. Нужен ли новый ADR: YES/NO.
21. Hardware writes: NONE.
22. Verdict:
    GPU MUTATION ARCHITECTURE READY /
    GPU MUTATION NEEDS MORE EVIDENCE /
    BLOCKED. 

---

## Assistant (Developer · GPT-5.6 Luna · 10.9s)

**Tool: skill**

---

## Assistant (Developer · GPT-5.6 Luna · 11.1s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 16.8s)

**Tool: read**

**Tool: bash**

**Tool: glob**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

---

## Assistant (Developer · GPT-5.6 Luna · 9.4s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 14.7s)

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: grep**

**Tool: glob**

---

## Assistant (Developer · GPT-5.6 Luna · 6.4s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 6.9s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 8.4s)

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 10.6s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 6.4s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 9.5s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 18.8s)

**Tool: bash**

**Tool: bash**

**Tool: bash**

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 109.8s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 15.7s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 14.8s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 16.9s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 12.1s)

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 14.5s)

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

**Tool: read**

---

## Assistant (Developer · GPT-5.6 Luna · 23.4s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 10.6s)

**Tool: bash**

---

## Assistant (Developer · GPT-5.6 Luna · 7.3s)

**Tool: todowrite**

---

## Assistant (Developer · GPT-5.6 Luna · 51.0s)

## GPU mutation readiness audit

### 1. Domain concepts

Actual domain model содержит независимые типы:

- `GpuMode::{Eco, Standard, Ultimate, Optimized}`;
- `GpuMuxState::{Integrated, Discrete, Unknown}`;
- `GpuAccessPolicy::{Unblocked, Blocked, Pending, Unknown}`;
- `GpuPowerState::{Active, Suspended, Off, Stale, Unknown}`;
- `ActionRequirement::{None, Logout, Reboot}` и `PendingAction`.

`GpuMode` — вариант **B/C**: product preset abstraction, сохранённая legacy-моделью. Это не единый hardware fact и не backend state. ADR 0005 запрещает объединять его с MUX/access/power.

`GpuState` legacy aggregate содержит requested mode, MUX, access, power и requirement, но production read path уже использует независимые capability providers.

---

### 2. Production UI

Product cards:

- `Eco`
- `Standard`
- `Ultimate`
- `Optimized`

В текущем production GUI они:

- disabled;
- `gpu_mode_state=Unavailable`;
- `gpu_mode_writable=false`;
- selected state не показывается как authoritative;
- показывается `GPU mode control unavailable`.

Отдельные read-only status cells:

- `Power`
- `MUX`
- `Access`

UI labels:

- Power: `Active`, `Suspended`, `Off`, `Stale`, `Unknown`;
- MUX: `Integrated`, `Discrete`, `Unknown`;
- Access: `Unblocked`, `Blocked`, `Pending`, `Unknown`.

При `Loading` отображается `…`, при backend absence/error — `Unavailable`.

Production mode cards не отправляют `SetGpuMode`; optimistic production GPU mode state отсутствует. Legacy mock/offscreen path сохраняет mode tests и pending behavior.

---

### 3. MUX read path

```text
GUI status cell
→ AppService::gpu_mux_state()
→ SessionGpuMuxProvider
→ Session1.GpuMux
→ orbis-sessiond ArmouryGpuProvider
→ /sys/class/firmware-attributes/asus-armoury/
  attributes/gpu_mux_mode/current_value
```

Host:

```text
current_value: 1
possible_values: 0;1
permissions: -rw-r--r-- root:root
mapping: 0 → Discrete, 1 → Integrated
```

Production current state: `Integrated`.

Semantics:

- fresh read on every provider call;
- no cache;
- absent attribute → `Unsupported`;
- malformed → `ProviderError::Internal`;
- unknown raw value → `GpuMuxState::Unknown`.

---

### 4. dGPU access/block read path

```text
GUI status cell
→ AppService::gpu_access_policy()
→ SessionGpuAccessProvider
→ Session1.GpuAccess
→ orbis-sessiond ArmouryGpuProvider
→ /sys/class/firmware-attributes/asus-armoury/
  attributes/dgpu_disable/current_value
```

Host:

```text
current_value: 0
possible_values: 0;1
permissions: -rw-r--r-- root:root
mapping: 0 → Unblocked, 1 → Blocked
```

Production current state: `Unblocked`.

Semantics:

- fresh read;
- no cache;
- absent → `Unsupported`;
- malformed → `Internal`;
- unknown raw → `GpuAccessPolicy::Unknown`.

---

### 5. dGPU power/runtime read path

```text
GUI status cell
→ AppService::gpu_power_state()
→ SessionGpuPowerProvider
→ Session1.GpuPower
→ orbis-sessiond SupergfxdGpuPowerProvider
→ system D-Bus org.supergfxctl.Daemon
→ /org/supergfxctl/Gfx
→ org.supergfxctl.Daemon.Power()
```

Installed production API result:

```text
Power() = 1
domain = Suspended
```

Current host runtime status for NVIDIA PCI functions is `suspended`.

Mapping proven from installed API:

```text
0 → Active
1 → Suspended
2 → Off
3 → AsusDisabled → Unknown
4 → Unknown → Unknown
future values → Unknown
```

Every read is fresh; no property cache.

---

### 6. Installed supergfxd

Version:

```text
supergfxctl/supergfxd 5.2.7
```

System D-Bus:

```text
bus: org.supergfxctl.Daemon
path: /org/supergfxctl/Gfx
interface: org.supergfxctl.Daemon
```

Read-only API:

```text
Version()
Mode()
Supported()
Vendor()
Power()
PendingMode()
PendingUserAction()
Config()
```

Signals:

```text
NotifyGfxStatus(u)
NotifyGfx(u)
NotifyAction(u)
PropertiesChanged(...)
```

Current read-only CLI evidence:

```text
Mode: Hybrid
Supported: [Integrated, Hybrid, AsusMuxDgpu]
Vendor: Nvidia
Power: suspended
PendingUserAction: No action required
PendingMode: Unknown
```

Mutation operations exposed by API:

```text
SetMode(u) -> u
SetConfig((ubbbbtu)) -> ...
```

No mutation was called.

The binary exposes lifecycle/action states including:

```text
Logout
Reboot
SwitchToIntegrated
AsusMuxDgpu
AsusEgpu...
```

It also contains staged lifecycle operations for display-manager stop/start, driver load/unload, PCI unbind/remove/rescan, NVIDIA services, VFIO and related transitions.

D-Bus policy allows users/groups to send calls, so supergfxd is itself the existing privileged compatibility owner. This does not eliminate the need to define Orbis caller/authorization policy.

---

### 7. Host ASUS/kernel interfaces

Existing Orbis-read interfaces:

| Concept | Host source | Current raw | Owner |
|---|---|---:|---|
| MUX | `.../gpu_mux_mode/current_value` | `1` | kernel ASUS Armoury firmware attribute |
| MUX choices | `.../gpu_mux_mode/possible_values` | `0;1` | kernel |
| dGPU access | `.../dgpu_disable/current_value` | `0` | kernel ASUS Armoury firmware attribute |
| access choices | `.../dgpu_disable/possible_values` | `0;1` | kernel |
| runtime power | supergfxd `Power()` | `1` | supergfxd |
| supporting runtime PM | `/sys/bus/pci/devices/0000:01:00.{0,1}/power/runtime_status` | `suspended` | kernel |

No writes were made.

The existence of writable-looking `0644` firmware-attribute files is not sufficient mutation proof. Current Orbis Armoury source deliberately opens them read-only and has no queued/pending write implementation.

---

### 8. Ownership

| Concept | Recommended owner |
|---|---|
| Runtime dGPU power state | supergfxd read API |
| MUX transitions | supergfxd compatibility owner, pending proof of exact mode semantics |
| dGPU access/block | ASUS Armoury/asusd or Cardwire depending selected contract; not Orbis yet |
| Full GPU lifecycle | supergfxd |
| Product mode policy | Orbis policy layer, but only after primitive mutation capabilities exist |

Do not reimplement supergfxd lifecycle in Orbis. Its MUX/mode transitions are coupled to logout, display manager, drivers, PCI, NVIDIA services, VFIO and reboot requirements.

---

### 9. Product mode mapping

| Mode | Desired MUX | Desired access | Power expectation | Backend/lifecycle | Status |
|---|---|---|---|---|---|
| Eco | likely iGPU / no MUX switch | dGPU blocked | likely suspended/off | `dgpu_disable`, Cardwire or supergfxd Integrated; process/display checks required | **UNKNOWN as complete mapping** |
| Standard | hybrid | dGPU unblocked/on-demand | runtime-dependent | supergfxd `Hybrid` candidate | primitive and policy contract not implemented in Orbis |
| Ultimate | dGPU MUX | dGPU available | active/available | supergfxd `AsusMuxDgpu` or ASUS MUX backend | reboot/action semantics required; no live mutation |
| Optimized | policy-dependent | battery→Eco, AC→Standard per ADR | policy-dependent | sessiond automation/policy | not a hardware mode; exact automation contract not implemented |

Important distinctions:

- `Ultimate` is not “dGPU power on”; it is a MUX/product request.
- `Eco` is not merely MUX state; it implies access/block and safety checks.
- `Optimized` is a policy/preset, not a proven supergfxd mode.
- No complete four-mode mapping is currently proven.

Per transition lifecycle classification:

- runtime power read: LIVE read-only;
- supergfxd `Hybrid`/`Integrated`/`AsusMuxDgpu`: mutation lifecycle requirements need dedicated contract evidence;
- MUX: likely reboot requirement, confirmed by local docs/evidence;
- Eco access block: live applicability and owner semantics UNKNOWN;
- Optimized: policy transition and debounce/pending semantics UNKNOWN.

---

### 10. Mutation boundary

Recommended architecture, if Orbis owns the user-facing mutation boundary:

```text
original GUI/application caller
→ Hardware1
→ Orbis polkit
→ minimal hardwared adapter
→ typed supergfxd D-Bus API
→ supergfxd lifecycle
→ fresh read-back
```

Reasons:

- preserve original caller identity;
- avoid direct privileged GUI/application calls;
- keep Orbis authorization explicit;
- avoid turning hardwared into a generic sysfs writer;
- delegate lifecycle to the existing supergfxd owner.

However, the exact Hardware1 contract, polkit actions, pending/action DTOs and read-back model are not yet designed. Direct application → supergfxd would weaken the established Orbis privilege-boundary pattern and should not be selected without a deliberate exception.

---

### 11. First mutation slice

No sufficiently safe complete product-mode slice is proven yet.

Most promising candidate for future investigation:

```text
read-only supergfxd Mode / PendingMode / PendingUserAction /
Supported / Config
```

followed by a narrowly scoped mutation contract for one supergfxd mode transition, likely `Hybrid`/`Integrated`, but this is **not yet selected for implementation** because:

- exact mode-to-primitive semantics need confirmation;
- pending/action result mapping needs domain types;
- logout/reboot/display-manager lifecycle needs bounded contract;
- authoritative post-transition read-back needs definition;
- no safe rollback contract has been established;
- live mutation would affect display/GPU lifecycle.

A lower-risk first implementation slice could instead be a **read-only pending/action capability provider**, but that is not a mutation slice.

Therefore:

```text
First mutation slice: NEEDS MORE EVIDENCE
```

No new ADR is required immediately; ADR 0005/0003 already establish the split and ownership direction. A new ADR will likely be needed before the first privileged GPU mutation if the selected supergfxd boundary introduces new authorization/pending semantics.

---

### 12. Tests/evidence

Relevant existing coverage:

- core enum/domain mapping tests;
- `GpuPowerProvider` independent provider tests;
- Armoury MUX/access mapping, unknown and fresh-read tests;
- supergfxd power mapping and fresh-read tests;
- Session1 GPU P2P/read tests;
- live ignored read-only tests for GPU power/MUX/access.

No mutation tests exist for a real supergfxd mode transition, and none were run.

---

### 13. Safety

- GPU mutation calls: `NONE`
- MUX writes: `NONE`
- `dgpu_disable` writes: `NONE`
- PCI remove/rescan: `NONE`
- driver/NVIDIA service manipulation: `NONE`
- logout/reboot/restart: `NONE`
- sudo: `NONE`
- Nix/build: `NONE`
- working tree unchanged.

## Verdict

**GPU MUTATION NEEDS MORE EVIDENCE**

---

## User

dsfds\

---

## User

Ты работаешь в существующем репозитории **Orbis / orbis-control**.

Это первая implementation-task после полного source-level аудита и архитектурного ревью.

## Цель

Привести архитектурную документацию проекта в соответствие с **фактически существующей реализацией** и зафиксировать принятое направление дальнейшей эволюции.

**Не менять runtime-код и поведение приложения в этой задаче.**

После выполнения следующая development-session должна однозначно понимать:

* что является текущей архитектурой;
* какие части legacy;
* какие решения остаются обязательными invariants;
* какие архитектурные изменения запланированы следующими;
* какие старые ADR были дополнены или частично superseded более поздними решениями.

## Source-of-truth

Соблюдай существующую hierarchy проекта:

### Current implementation facts

1. Production code и tests.
2. `docs/current-state.md`.
3. `docs/research/orbis-source-audit.md`.

### Architecture

1. Accepted ADR.
2. `docs/architecture.md`.

### Hardware facts

1. Dated live evidence.
2. Fixtures.
3. Historical research.

### Future direction

`docs/roadmap.md`.

Не переписывай historical hardware evidence под сегодняшнее состояние.

---

# Архитектурный verdict, который нужно зафиксировать

## KEEP

Следующие решения остаются фундаментальными:

* Rust + Slint;
* normal-user GUI;
* `orbis-sessiond` как непривилегированный user daemon;
* узкий `orbis-hardwared`;
* typed D-Bus boundaries;
* direct original caller → `Hardware1` для privileged interactive mutations;
* polkit authorization по original `system-bus-name`;
* отсутствие mutation delegation через `Session1`;
* authoritative read-back;
* отсутствие optimistic hardware state;
* distinction:

  * Unknown;
  * Unsupported;
  * Unavailable;
  * ReadOnly;
  * PermissionDenied;
* capability-driven architecture;
* evidence-driven hardware claims;
* отсутствие mock fallback в production;
* отсутствие generic root/sysfs writer;
* GPU concepts остаются независимыми:

  * product/requested policy;
  * physical MUX;
  * access policy;
  * runtime power;
  * pending/action requirement.

## EVOLVE

Следующие области требуют дальнейшего архитектурного развития:

1. **Runtime Capability Registry**

   * существующий `orbis-capabilities` пока не является полноценным runtime discovery system;
   * capabilities должны строиться из фактических providers/probes;
   * read/write/constraints/backend/requirements должны моделироваться отдельно.

2. **Application/worker composition**

   * текущий `run_worker` уже принимает слишком много независимых services;
   * дальнейшее добавление telemetry/fans/display/etc. не должно просто увеличивать число параметров;
   * нужен отдельный последующий refactor.

3. **GPU architecture**

   * concept-specific provider traits являются целевым направлением;
   * legacy monolithic `GpuProvider` остаётся временным migration artifact;
   * product `GpuMode` должен в будущем быть policy layer, а не raw hardware/backend enum.

4. **State architecture**
   В дальнейшем требуется явное разделение:

   * ObservedState;
   * DesiredState;
   * PendingState;
   * CapabilityState.

5. **Telemetry**

   * authoritative configuration state и telemetry имеют разные semantics;
   * configuration reads остаются fresh/authoritative;
   * telemetry в будущем может использовать timestamped samples/subscriptions;
   * telemetry cache не должен трактоваться как authoritative configuration cache.

6. **Provider selection**

   * не фиксировать один глобальный порядок backend для всего приложения;
   * ownership/provider priority определяется **per capability**.

Примеры:

```text
Performance:
kernel platform_profile

Battery configured threshold:
asusd

Battery effective threshold:
kernel power_supply

Battery general telemetry:
UPower

GPU staged lifecycle:
supergfxd compatibility backend
```

Одна product capability может использовать несколько authoritative sources для разных semantics.

## REPLACE / REMOVE LATER

Не делать этот refactor сейчас, но явно отметить future direction:

* legacy `GpuProvider` должен быть завершённой миграцией заменён capability-specific interfaces;
* production composition не должна зависеть от `MockProvider`;
* `MockProvider` остаётся для tests/offscreen/deterministic scenarios;
* growing `run_worker(...services...)` composition должна быть заменена;
* dead module options и устаревшие helpers должны быть удалены отдельными mechanical tasks после проверки usages.

## DEFER

Не объявлять готовыми и не проектировать write implementation без evidence:

* power limits;
* окончательный product GPU Eco/Standard/Ultimate/Optimized mapping;
* live GPU mutation;
* automation privilege semantics.

---

# Особо важное уточнение sessiond / hardwared

Зафиксировать актуальную architecture:

## Read path

```text
GUI
→ application
→ Session1
→ orbis-sessiond
→ authoritative read provider
→ kernel/system service
```

## Interactive privileged mutation

```text
original application caller
→ Hardware1
→ orbis-hardwared
→ polkit(original system-bus-name)
→ bounded mutation backend
→ read-back
```

`orbis-sessiond` **не является privileged mutation deputy**.

Не возвращать архитектуру к:

```text
GUI → Session1 → sessiond → Hardware1
```

для interactive privileged writes.

---

# hardwared semantics

Зафиксировать, что `orbis-hardwared`:

* существует;
* входит в фактическую production architecture;
* является узким root helper;
* не generic hardware daemon;
* не принимает arbitrary path/string/command;
* расширяется по одной доказанной privileged capability;
* каждая новая capability требует:

  * semantic typed API;
  * validation;
  * authorization;
  * backend ownership decision;
  * authoritative read-back;
  * tests/evidence.

Текущие доказанные mutation paths:

* Performance;
* Battery.

GPU staged architecture существует, но live mutation ещё не считается реализованной.

---

# ADR history

Не переписывай исторический смысл старых ADR так, будто первоначальное решение было другим.

В частности, ADR 0002 был корректным для момента его принятия.

Вместо изменения истории:

* добавь явную пометку о том, что его положение об отсутствии `hardwared` было **частично superseded / amended** более поздними ADR;
* сослаться на соответствующие более поздние решения;
* проверить все существующие ADR и определить их фактическое состояние.

Не придумывай номера ADR.

Если для этого architecture review нужен новый ADR:

1. сначала найди фактический максимальный ADR number в repository;
2. используй следующий свободный номер;
3. создай ADR с названием примерно:

```text
Architecture Evolution after Source Audit
```

или более точным названием после анализа.

Новый ADR должен фиксировать только значимые architectural decisions, а не пересказывать audit.

---

# Документы для проверки

Минимально проверить:

```text
AGENTS.md
docs/README.md
docs/architecture.md
docs/current-state.md
docs/roadmap.md
docs/threat-model.md
docs/provider-matrix.md
docs/adr/*
docs/research/orbis-source-audit.md
```

Также проверь ссылки из этих документов.

## architecture.md

Обновить так, чтобы он описывал **current architecture**, а не старое промежуточное состояние.

Особенно проверить:

* membership `orbis-hardwared` в workspace;
* его текущие capabilities;
* Battery mutation;
* Performance mutation;
* GPU primitive reads;
* Session1 vs Hardware1;
* direct-caller authorization;
* production Mock usage;
* current worker composition.

## docs/README.md

Проверить список ADR.

Он должен перечислять все реально accepted/current ADR и правильно описывать их роль.

## AGENTS.md

Source audit обнаружил утверждение, что `orbis-hardwared` не входит в workspace.

Проверь фактический файл и исправь только если contradiction всё ещё существует.

## threat-model.md

Не переписывать целиком.

Обновить только явно устаревшие архитектурные положения, в частности actual Hardware1/original-caller model и текущий hardwared status.

Future mitigations должны оставаться маркированными как future, если они ещё не реализованы.

## roadmap.md

Устранить внутренние stale status claims.

Например, не должно одновременно оставаться утверждение, что Battery mutation pending, если `current-state.md` и live evidence уже фиксируют её как completed.

Не менять будущий priority целиком в этой task.

Добавить следующий архитектурный этап/направление только если это требуется для согласованности:

```text
Architecture consolidation:
- application composition refactor
- runtime Capability Registry
- production Mock removal
- GPU split migration
```

Без реализации этих пунктов.

---

# Не делать

В этой task запрещено:

* менять Rust runtime logic;
* менять Slint UI;
* менять D-Bus wire API;
* делать worker refactor;
* внедрять Capability Registry;
* удалять legacy GpuProvider;
* удалять MockProvider;
* реализовывать telemetry;
* реализовывать fan control;
* реализовывать GPU mutation;
* выполнять hardware writes;
* менять polkit policy;
* менять systemd services;
* менять Nix package behaviour.

Это docs/architecture normalization task.

---

# Проверка

Поскольку task documentation-only:

Обязательно:

```bash
git diff --check
```

Также:

* проверить Markdown links, насколько это возможно существующими repo tools;
* grep/search на stale architecture claims:

  * `hardwared` not in workspace;
  * hardwared only Performance;
  * Battery mutation pending;
  * Session1 mutation delegation;
* проверить ADR index;
* проверить, что ссылки на superseding ADR корректны.

Cargo/Nix full suite запускать не требуется, если code/package files не изменялись.

Если repository policy требует дополнительные docs checks — выполнить их.

---

# Итог задачи

В конце сообщи:

1. какие файлы изменены;
2. был ли создан новый ADR и какой;
3. какие contradictions устранены;
4. какие historical ADR помечены amended/superseded;
5. результат `git diff --check`;
6. были ли найдены новые существенные архитектурные противоречия;
7. полный `git diff --stat`.

Не начинай следующую implementation-task.

После завершения остановись. 

---

