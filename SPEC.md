# Orbis Control — SPEC

## 1. Цель продукта

Orbis Control — десктопное приложение для Arch Linux, которое предоставляет единый интерфейс управления поддерживаемыми аппаратными возможностями ноутбуков ASUS ROG/TUF/Zephyrus.

Цель текущей версии — довести существующий проект до состояния, в котором основные функции управления производительностью, охлаждением и GPU работают end-to-end, честно отражают фактическое состояние оборудования и доступны через аккуратный, предсказуемый UI.

Приоритет:

1. фактически работающий пользовательский сценарий;
2. честное отображение hardware state;
3. безопасное применение изменений;
4. удобный и визуально аккуратный интерфейс;
5. сохранение существующего рабочего кода и архитектуры.

Проект не переписывается с нуля.

## 2. Целевая платформа

Текущая версия ориентирована на:

- Arch Linux;
- Wayland как основной desktop environment;
- существующий Rust workspace;
- существующий Slint UI;
- существующие `orbis-sessiond`, `orbis-hardwared`, D-Bus и polkit boundaries;
- runtime capability detection вместо предположений по модели ноутбука.

ASUS FA707NV может использоваться как основной real-hardware validation target, но production-код не должен быть жёстко привязан к конкретной модели.

## 3. Scope текущей версии

### 3.1. Профили производительности

Пользователь должен:

- видеть текущий platform performance profile;
- видеть только реально доступные профили;
- переключаться между доступными профилями;
- видеть состояние применения;
- после записи получать подтверждённое фактическое состояние.

UI должен опираться на фактически обнаруженные возможности, а не на предполагаемый набор профилей.

### 3.2. Кривые вентиляторов CPU/GPU

Пользователь должен:

- видеть текущие RPM CPU/GPU fan, если доступны;
- выбирать CPU или GPU fan;
- выбирать поддерживаемый профиль вентилятора;
- видеть сохранённую кривую;
- редактировать точки кривой;
- видеть Dirty state до применения;
- явно применять изменения;
- после применения получать authoritative read-back;
- восстанавливать заводские значения только при доказанной reset capability;
- видеть enabled/disabled state custom curve, если backend предоставляет его.

Существующие fan backend, domain types и editor необходимо переиспользовать. Новый software fan controller в scope не входит.

### 3.3. Power / thermal limits

Пользователь должен видеть и изменять только те поля, которые реально обнаружены backend.

Поддерживаемые доменные поля:

- SPL / PPT PL1;
- SPPT / PPT PL2;
- FPPT / PPT PL3;
- CPU temperature limit;
- NVIDIA Dynamic Boost;
- GPU temperature target;
- другие typed power-limit поля только при наличии реального backend support.

Каждое поле должно иметь:

- текущее фактическое значение;
- единицу измерения;
- minimum;
- maximum;
- step;
- default, если backend способен его доказать;
- read/write capability.

Не реализовывать один синтетический «TDP» slider, если backend предоставляет несколько независимых power limits. Допустима группа «Лимиты мощности», но внутри должны оставаться реальные отдельные параметры.

Диапазоны и шаги нельзя придумывать или жёстко задавать в UI, если они доступны из authoritative backend metadata.

### 3.4. Boost controls

Нужно различать:

1. platform profile `Turbo`;
2. NVIDIA Dynamic Boost;
3. CPU hardware turbo/boost enable/disable.

Это разные функции.

NVIDIA Dynamic Boost реализуется через существующую модель power limits, если capability доступна.

CPU Boost toggle реализуется только при наличии доказанного authoritative Linux/ASUS read/write interface. Если такого интерфейса нет, нельзя придумывать shell workaround или выдавать platform profile Turbo за CPU Boost.

### 3.5. GPU modes / MUX

Использовать существующую модель GPU product modes и MUX state.

Пользователь должен:

- видеть текущий GPU product mode;
- видеть доступные режимы;
- выбирать поддерживаемый режим;
- видеть queued/pending mode;
- видеть необходимость reboot/shutdown;
- отдельно видеть physical MUX state;
- отдельно видеть dGPU access state;
- отдельно видеть dGPU runtime power state, если он доступен.

Не создавать второй независимый MUX subsystem. Если изменение режима только ставит transition в очередь, UI не должен сообщать, что физический MUX уже переключён.

### 3.6. Уже существующий функционал

Работающие функции проекта не должны ломаться в результате текущей итерации. В частности, где уже поддерживается:

- battery charge limit;
- telemetry;
- keyboard backlight;
- panel overdrive;
- Aura/static RGB;
- tray/window lifecycle;
- preferences;
- diagnostics.

Расширение этих функций не входит в scope, кроме исправления regressions от текущих изменений.

## 4. Вне scope

В текущую версию не входят:

- смена UI framework;
- переписывание приложения с нуля;
- новая daemon architecture;
- generic privileged shell/sysfs proxy;
- generic privileged command runner;
- software-managed closed-loop fan controller;
- automation engine;
- автоматическое переключение профилей по приложениям;
- per-game profiles;
- cloud sync;
- account system;
- remote control;
- import/export пользовательских performance presets;
- display refresh mutation;
- собственный updater;
- AppImage/Debian/Fedora packaging;
- поддержка произвольных не-ASUS ноутбуков;
- масштабный cleanup всего workspace;
- нерелевантный refactoring;
- функции только потому, что они есть в G-Helper, G-Helper-Linux, LACT или ROG Control.

Референсные приложения используются как UX/reference material, а не как требование копировать их полностью.

## 5. Основные пользовательские сценарии

### US-01. Смена профиля производительности

Пользователь видит текущий performance profile и переключает его. После операции приложение показывает профиль, фактически прочитанный обратно из системы.

### US-02. Настройка охлаждения

Пользователь выбирает CPU или GPU и профиль, изменяет точки кривой и явно нажимает Apply. До Apply hardware state не меняется. После применения UI обновляется по read-back.

### US-03. Настройка лимитов мощности

Пользователь открывает «Производительность» и видит только реально доступные SPL/SPPT/FPPT. Для каждого параметра отображаются значение и единица. После изменения параметр валидируется, применяется и перечитывается.

### US-04. Настройка thermal/boost limits

Пользователь видит только доступные CPU temperature limit, GPU temperature target, GPU Dynamic Boost и CPU Boost, если он действительно поддержан отдельной capability.

### US-05. Переключение GPU/MUX mode

Пользователь выбирает реально поддерживаемый GPU mode. Если требуется reboot/shutdown, UI показывает Pending/Queued state и не подменяет его Applied state.

### US-06. Понимание причины недоступности

Для hardware controls должны различаться состояния Loading, Read-only, Unsupported, Temporarily unavailable, Authorization denied, Apply failed и Unknown outcome.

## 6. Функциональные требования

### FR-001. Capability discovery

Каждый hardware control определяется runtime evidence. Read capability и write capability определяются отдельно. Нельзя включать функцию только по DMI model name.

### FR-002. Initial loading

При запуске приложение не показывает фиктивные hardware defaults как реальные значения. До authoritative read control находится в Loading/Unknown state.

### FR-003. External changes

Если hardware state изменён внешним инструментом, следующий refresh должен привести UI к фактическому observed state. Локально сохранённое значение не является источником истины.

### FR-010. Performance profile

При выборе profile:

1. validate request;
2. выполнить существующий typed mutation path;
3. получить результат;
4. выполнить authoritative read-back;
5. обновить observed state.

При mismatch requested state не отображается как Applied.

### FR-020. Fan curve identity

Fan curve определяется как минимум сочетанием fan identity, fan profile, curve points и enabled state, если он известен backend. Существующие различия backend profiles нельзя терять при UI conversion.

### FR-021. Fan curve editing

Редактирование curve является локальным draft, не пишет hardware при каждом движении точки, устанавливает Dirty state и применяется только явным действием пользователя.

### FR-022. Fan curve validation

Перед записью использовать существующую backend/domain validation. Количество точек, допустимые температуры, PWM и monotonic rules берутся из backend contract.

### FR-023. Unsaved fan changes

При Dirty state смена fan/profile не должна молча терять изменения. Минимально допустимо confirmation dialog с вариантами остаться или отбросить draft. Автоматическое применение запрещено.

### FR-024. Fan reset

Factory reset доступен только при доказанной write capability и требует read-back. Malformed vendor curve должна приводить к ошибке, а не к fake defaults.

### FR-030. Power limit metadata

Каждый power/thermal field получает от backend current value, minimum, maximum, step, unit и optional default. UI не является источником этих ограничений.

### FR-031. Power limit validation

Нельзя отправлять значение ниже minimum, выше maximum или не соответствующее step, если step alignment обязателен.

### FR-032. Relationships between limits

Не вводить самостоятельно правила вроде `SPL <= SPPT <= FPPT`, если это не является requirement authoritative backend.

### FR-033. Power limit apply

Изменение power/thermal field остаётся draft/pending до явного Apply, проходит validation, записывается через typed privileged path, перечитывается и только после подтверждения становится observed value.

### FR-040. CPU Boost

CPU Boost является отдельной capability. Если authoritative read/write mechanism отсутствует, production control не создаётся.

### FR-041. NVIDIA Dynamic Boost

GPU Dynamic Boost отображается как отдельный числовой hardware limit с реальными unit/min/max/step и не заменяет platform Turbo profile.

### FR-050. GPU mode

Current product mode, queued mode и physical state отображаются раздельно.

### FR-051. Reboot-required transition

При reboot-required выбранный mode отображается как Pending/Queued, physical MUX остаётся текущим observed state, UI показывает необходимость reboot/shutdown.

### FR-060. Privilege boundary

GUI остаётся unprivileged. Hardware writes выполняются только через существующий narrow typed privileged boundary.

Запрещены `sudo` из GUI, generic command execution API, generic privileged file writer, generic sysfs proxy и arbitrary D-Bus forwarding.

### FR-061. Authorization failure

Отказ polkit не меняет observed state, снимает indefinite loading, показывает operation error и позволяет повторить действие вручную.

### FR-062. Unknown mutation outcome

Если mutation могла быть отправлена, но response потерян или произошёл timeout, outcome считается Unknown. Mutation нельзя автоматически повторять; сначала выполняется fresh read.

## 7. Логическая модель состояния

Не создавать новые архитектурные сущности, если существующие domain types и `UiState` уже способны представить состояние.

### PerformanceState

- available profiles;
- observed profile;
- writable;
- loading/error state;
- pending request.

### FanCurveState

- fan id;
- fan profile;
- observed curve;
- draft curve;
- enabled state / unknown;
- writable;
- dirty;
- mutation/reset capability;
- loading/error.

### PowerLimitState

Для каждого `PowerLimitField`:

- field identity;
- observed value;
- draft value;
- min;
- max;
- step;
- default optional;
- unit;
- readable;
- writable;
- pending/error state.

### GpuModeState

- available product modes;
- observed product mode;
- queued product mode optional;
- reboot-required;
- physical MUX state;
- dGPU access state;
- dGPU runtime power state;
- writable;
- pending/error state.

## 8. Persistence

Через существующую persistence system сохраняются только пользовательские настройки приложения, для которых persistence уже предусмотрен.

Observed hardware state не сохраняется как authoritative truth.

Нельзя автоматически применять при старте сохранённые fan curves, power limits, temperature limits, boost values или GPU mode только потому, что они присутствуют в config. Loading config не должен сам по себе мутировать hardware.

Cloud sync и import/export performance presets отсутствуют.

## 9. UX/UI

### 9.1. Общая структура

Сохранить существующую single-window structure: preferred 1200×800, minimum 980×680, sidebar, card-based sections, существующие dark/light themes и reusable Slint components.

### 9.2. Основные экраны

**Dashboard:** краткая сводка доступных temperatures, fan RPM, performance profile, battery/power и GPU state.

**Performance:** platform profile, power limits, thermal/boost limits.

**Cooling:** CPU/GPU selector, profile selector, curve editor, Dirty/Applied/Error state, Apply, Factory reset при поддержке.

**Graphics:** product GPU modes, Pending/Reboot state, physical MUX, dGPU access, runtime dGPU power.

### 9.3. Control states

Каждый hardware block должен визуально различать Loading, Ready writable, Ready read-only, Unsupported, Temporarily unavailable, Dirty, Applying, Pending reboot и Error.

### 9.4. Visual consistency

Существующий audited UI является базовой design system.

Требования:

- использовать существующие Palette/components;
- одинаковые controls должны выглядеть одинаково;
- не вводить случайные локальные цвета, радиусы и spacing;
- сохранять единые card padding и page margins;
- элементы одной группы выравнивать по общей сетке;
- длинный русский текст не должен перекрывать controls;
- primary pages не должны требовать horizontal scroll;
- vertical scroll допустим;
- изменение размеров окна не должно приводить к overlap;
- Disabled, Dirty, Pending и Error должны визуально различаться.

### 9.5. Обязательные visual states

Зафиксировать screenshots для:

1. Dashboard — normal loaded;
2. Performance — normal supported;
3. Performance — часть advanced controls unsupported/read-only;
4. Cooling — CPU curve normal;
5. Cooling — GPU curve Dirty;
6. Cooling — apply error;
7. Graphics — current mode normal;
8. Graphics — queued mode / reboot required;
9. hardware screen — backend unavailable;
10. main window — 980×680;
11. dark theme;
12. light theme.

Использовать существующий snapshot mechanism/fixture path, где возможно. Масштабный redesign требует отдельного решения пользователя.

## 10. Edge cases и ошибки

Обязательно обрабатывать:

- session daemon отсутствует;
- hardware daemon отсутствует;
- backend появляется после запуска;
- backend пропадает во время работы;
- capability существует только для read;
- capability полностью отсутствует;
- permission denied;
- malformed backend value;
- value изменён внешним приложением;
- mutation timeout с неизвестным исходом;
- read-back отличается от requested;
- GPU mode уже соответствует requested;
- GPU mode queued, но ещё не applied;
- reboot-required;
- malformed fan factory curve;
- смена fan/profile при Dirty draft;
- min/max меняются после driver/backend update;
- GPU power недоступен, хотя MUX доступен;
- physical MUX state и requested product mode временно различаются.

Нельзя заменять эти случаи fake success или fake default.

## 11. Platform/system constraints

- Arch Linux — primary target.
- Production GUI запускается без root.
- systemd/D-Bus/polkit assets должны ссылаться на фактические установленные binaries.
- Все privileged mutations проходят через existing typed hardware service.
- Wayland остаётся основной сессией.
- Поддержка функции определяется runtime evidence.
- Real hardware writes выполняются только с явным разрешением владельца.
- Отсутствие разрешения на live writes не блокирует implementation и fake/P2P/integration tests.

## 12. Definition of Done

Версия считается готовой, если:

- применимые сценарии `ACCEPTANCE.md` проходят;
- profile switching работает end-to-end;
- CPU/GPU fan curves работают на поддержанном backend;
- power/thermal limits отображаются capability-driven;
- writable limits применяются с read-back;
- GPU/MUX flow сохраняет Pending/Reboot semantics;
- Unsupported/Read-only/Error не выглядят как success;
- нет overlap/clipping на целевых visual states;
- существующие ключевые функции не получили regression;
- Arch install запускает GUI и необходимые services;
- `scripts/verify task` проходит;
- release build проходит;
- все доступные проверки реально выполнены;
- непроверенные real-hardware возможности не обозначены как live-validated.
