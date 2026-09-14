# Orbis Control — IMPLEMENTATION PLAN

## Общие правила

Работа идёт по этапам ниже.

Если существующий код уже полностью выполняет этап:

1. доказать это кодом и проверками;
2. не переписывать рабочее решение;
3. выполнить acceptance checks;
4. перейти к следующему этапу.

После каждого этапа:

- workspace должен собираться;
- приложение должно оставаться запускаемым;
- targeted tests должны проходить;
- выполнить подходящий `scripts/verify crate ...`, `scripts/verify quick` или эквивалент;
- перед commit проверить diff;
- делать небольшие логичные commits;
- продолжать следующий этап без запроса «что делать дальше», если нет реального blocker.

Real hardware writes не выполнять без отдельного явного разрешения пользователя.

## Этап 0. Чистый baseline и фактическое состояние main

### Цель

Получить безопасный воспроизводимый starting point и перестать зависеть от неизвестного состояния старого local checkout.

### Что сделать

1. Проверить текущий working directory:
   - `pwd`;
   - `git status`;
   - current branch;
   - HEAD;
   - remotes.
2. Не удалять и не перезаписывать неизвестный local repository.
3. Если текущий checkout не доказанно чистый и синхронизированный с GitHub `main`, создать новый sibling checkout из `https://github.com/arttvad9r/Orbis-control`.
4. Старый checkout оставить нетронутым как backup.
5. В свежем repository:
   - обновить `main`;
   - создать отдельную feature branch;
   - прочитать `AGENTS.md`;
   - прочитать `SPEC.md`;
   - прочитать `ACCEPTANCE.md`;
   - прочитать `IMPLEMENTATION_PLAN.md`;
   - прочитать `DECISIONS.md`.
6. Определить, какие acceptance requirements уже реализованы.
7. Запустить baseline verification.

### Сейчас не трогать

- architecture;
- UI redesign;
- unrelated cleanup;
- старый local checkout;
- real hardware state.

### Критерии готовности

Известны:

- exact baseline commit;
- baseline build status;
- baseline test status;
- baseline UI snapshot status;
- какие requested controls реально уже wired.

### Проверки

Минимум:

```bash
scripts/verify task
cargo build --workspace --release --locked
```

Если доступен существующий snapshot example — отрендерить текущие UI states.

Если baseline failing, сначала определить root cause и устранить только реальный blocker для дальнейшей работы.

---

## Этап 1. Довести существующие CPU/GPU fan curves до production UI

### Цель

Получить законченный сценарий:

read curve → edit → Dirty → Apply → read-back.

### Что реализовать

1. Проследить существующий flow через UI, worker/runtime, session read, Hardware1 mutation и read-back.
2. Проверить, почему production UI может всё ещё выставлять safety block при уже существующем validated backend.
3. Заменить hard-coded UI block на реальное runtime capability/write-status state.
4. Проверить CPU и GPU независимо.
5. Сохранить существующую typed validation.
6. Реализовать безопасное поведение Dirty при смене fan/profile.
7. Factory reset разрешать только при доказанной capability.

### Сейчас не трогать

- новый software fan controller;
- EMA/hysteresis runtime policy;
- fan automation;
- unrelated Cooling redesign.

### Критерии готовности

Проходят:

- AC-020;
- AC-021;
- AC-022;
- AC-023;
- AC-024.

### Проверки

- существующие fan unit tests;
- session fan P2P tests;
- hardwared fan tests без real hardware mutation;
- affected UI tests;
- `scripts/verify crate orbis-ui`;
- `scripts/verify crate orbis-sessiond`;
- `scripts/verify crate orbis-hardwared`;
- `scripts/verify quick`.

---

## Этап 2. Power/thermal limits — read-only vertical slice

### Цель

Пользователь видит реальные доступные power/thermal controls и metadata без нового write path.

### Что реализовать

1. Найти и переиспользовать существующие `PowerLimitField`, `PowerLimitValue`, `PowerLimits`, `PowerLimitProvider`.
2. Определить authoritative backend для каждого доступного поля.
3. Получить read capability для поддерживаемых SPL, SPPT, FPPT, CPU temp limit, GPU Dynamic Boost и GPU temp target.
4. Не считать все поля обязательными.
5. Передать snapshot до UI существующим минимальным путём.
6. На Performance page добавить группу advanced power/thermal limits.
7. Для каждого поля использовать metadata backend.

### Сейчас не трогать

- write API;
- generic hardware mutation framework;
- presets;
- automation;
- единый fake TDP slider.

### Критерии готовности

Проходят:

- AC-002;
- AC-003;
- AC-030;
- AC-033.

### Проверки

- core metadata validation tests;
- provider tests на fixtures;
- session/P2P read test;
- UI supported/unsupported snapshots;
- `scripts/verify task`.

---

## Этап 3. SPL/SPPT/FPPT mutation

### Цель

Законченный пользовательский сценарий CPU power limits:

read → edit → validate → authorize → write → read-back.

### Что реализовать

1. Сначала найти существующий write path.
2. Если его нет, добавить только необходимые typed mutation methods через existing Hardware1 boundary.
3. Использовать authoritative min/max/step.
4. Сохранить разделение requested/observed/pending.
5. Добавить authorization только для конкретных typed operations.
6. После write выполнять read-back.
7. Обработать permission denied, unsupported, out-of-range, timeout/unknown outcome и read-back mismatch.

### Сейчас не трогать

- generic sysfs writer;
- generic ASUS attribute mutation API;
- automatic startup apply;
- arbitrary relationships SPL/SPPT/FPPT без backend evidence.

### Критерии готовности

Проходят:

- AC-031;
- AC-032;
- AC-034;
- AC-035.

### Проверки

- domain validation tests;
- provider/backend tests;
- private D-Bus/P2P Hardware1 tests;
- authorization/error tests;
- UI mutation-state tests;
- `scripts/verify task`.

Real hardware validation — только после отдельного разрешения пользователя.

---

## Этап 4. Boost и temperature target mutation

### Цель

Закончить advanced performance controls без смешивания разных boost concepts.

### Что реализовать

Вертикально, используя pattern этапа 3:

1. NVIDIA Dynamic Boost.
2. GPU temperature target.
3. CPU temperature limit, если capability существует.
4. CPU Boost toggle — только после поиска authoritative существующего interface.

Для каждого:

read → metadata → UI → validate → typed write → read-back.

Если CPU Boost authoritative interface не найден:

- production toggle не изобретать;
- зафиксировать Unsupported;
- продолжить остальные пункты этапа.

### Сейчас не трогать

- platform Turbo semantics;
- shell hacks;
- undocumented MSR writes;
- generic CPU tuning framework.

### Критерии готовности

Проходят:

- AC-040;
- AC-041;
- применимые AC-031/032/034/035.

### Проверки

Те же уровни, что в этапе 3, плюс regression performance-profile tests.

---

## Этап 5. GPU modes / MUX convergence

### Цель

Не создавать новый GPU subsystem, а убедиться, что существующий product-mode/MUX flow полностью соответствует product UX.

### Что реализовать

1. Проверить current product GPU mode implementation.
2. Проверить current mode, queued mode, reboot required, physical MUX, dGPU access и dGPU runtime power.
3. Исправить только реальные gaps.
4. Убедиться, что Ultimate/MUX transition не отображается как Applied до фактического transition.
5. Проверить независимые Unavailable states.

### Сейчас не трогать

- второй MUX backend;
- wake dGPU ради telemetry;
- display modesetting;
- новый GPU abstraction layer.

### Критерии готовности

Проходят:

- AC-050;
- AC-051;
- AC-052.

### Проверки

- existing GPU capability tests;
- product GPU P2P tests;
- queued/reboot UI tests;
- Graphics screenshots;
- `scripts/verify task`.

---

## Этап 6. Целенаправленный UI polish

### Цель

Устранить visual defects без перепроектирования продукта.

### Что реализовать

Проверить Dashboard, Performance, Cooling и Graphics во всех обязательных visual states.

Исправлять:

- overlap;
- clipping;
- неправильное выравнивание;
- разные padding/spacing у одинаковых components;
- неконсистентные button/control sizes;
- длинные подписи;
- Disabled/Loading/Error/Pending states;
- layout на 980×680;
- dark/light theme regressions.

Переиспользовать существующие components и Palette. Если одинаковый дефект повторяется, исправлять общий component, а не копировать patch по страницам.

### Сейчас не трогать

- общий visual language;
- navigation model;
- полный redesign;
- новые UI libraries.

### Критерии готовности

Проходят:

- AC-080;
- AC-081.

### Проверки

- существующий UI snapshot mechanism;
- 980×680;
- 1200×800;
- Dark;
- Light;
- Normal;
- Dirty;
- Pending;
- Error;
- Unsupported/Read-only.

Выполнить ручной visual review screenshots.

---

## Этап 7. Arch integration и regression gate

### Цель

Получить устанавливаемое приложение, а не только working `cargo run`.

### Что реализовать

1. Проверить canonical Arch install path.
2. Проверить соответствие binary paths и systemd units.
3. Проверить D-Bus policy.
4. Проверить polkit actions.
5. Проверить desktop entry/icon/assets.
6. Выполнить clean install smoke.
7. Выполнить regression существующих production-ready features.
8. Проверить final git diff.

### Сейчас не трогать

- Debian/Fedora packaging;
- AppImage;
- self-update;
- unrelated feature expansion.

### Критерии готовности

Проходят:

- AC-001;
- AC-070;
- AC-071;
- AC-090;
- AC-100;
- AC-110.

### Проверки

```bash
scripts/verify task
cargo build --workspace --release --locked
scripts/verify full
```

Плюс canonical Arch install smoke на доступной environment.

---

## Final checkpoint

Перед объявлением версии готовой:

1. пройти `ACCEPTANCE.md`;
2. убедиться, что каждый новый write path имеет validation и read-back;
3. проверить отсутствие hard-coded fake support;
4. проверить отсутствие новых generic privileged interfaces;
5. проверить UI snapshots;
6. проверить `git status`;
7. просмотреть полный diff ветки относительно `main`;
8. проверить отсутствие случайных generated/debug files;
9. перечислить всё, что не получило real-hardware validation.
