# Orbis Control UI Redesign — единое окно, 4 раздела, Catppuccin

Дата: 2026-08-24. Статус: утверждено владельцем (структура A, Catppuccin, frameless).
Исходный контекст: #128, итерация «Orbis Control Center» (`f2624be`) признана
владельцем недостаточной; решение — отказ от ориентации на клон G-Helper и
глобальный редизайн UI-слоя.

## 1. Решения владельца (фиксируются как требования)

1. Одно окно с **сайдбаром из 4 разделов**: Dashboard, Fans, Hardware, Settings.
2. **Updates** — удалить из UI (для реальных обновлений нужен сервер; текущая
   поверхность только классифицирует «установка не разрешена»).
3. **Automation** — удалить из UI (unattended execution остаётся за safety-gate;
   поверхность не даёт пользовательской ценности). Rust-слой automation
   (shadow/recovery) не трогаем — исчезает только UI-раздел.
4. **Diagnostics** — не раздел: блок «Diagnostics» в Settings с кнопками
   Refresh / Copy / Export (существующие пути #111 сохраняются; Open Logs
   остаётся отключённым).
5. **Catppuccin** вместо текущей палитры: тёмная Mocha (по умолчанию),
   светлая Latte. Выбор темы остаётся в Settings.
6. **Frameless-окно**: нативная рамка/заголовок Wayland убираются; свой
   титлбар с drag-областью и кнопками свернуть/закрыть; скруглённые углы.
7. **Иконки**: небольшой набор монохромных line-SVG в `ui/assets/icons/`
   (dashboard, fan, cpu/hardware, gear, minimize, close). Без эмодзи.
8. Компактность: никаких больших пустых карточек; строки и сегменты;
   выпадающие списки вместо рядов кнопок там, где выбор >2 вариантов.

## 2. Информационная архитектура

Окно 760×600 logical px (фиксированное). Слева сайдбар 168px, справа контент
раздела. Свойства `UiState` и все существующие колбэки сохраняются; меняется
только композиция UI: один `AppWindow` со свойством `active-section` (int).

### 2.1 Dashboard

Только быстрые ежедневные контролы, компактные строки:

- **Performance** — `SegmentedTrack` (Silent/Balanced/Turbo) + телеметрия
  (CPU temp · fan RPM) в заголовке секции; unavailable/loading строки как сейчас.
- **GPU Mode** — `SegmentedTrack` (Eco/Standard/Ultimate/Optimized) + pending/
  error `LocalStatus`; телеметрия в заголовке.
- **Display** — строка `DropdownRow`: label + ComboBox (Auto/60/120), disabled
  без write evidence; статус в конце строки.
- **Keyboard** — строка `DropdownRow`: label + ComboBox (Off/1/2/3).
- **Battery** — одна строка: label + `ValueSlider` (компактный) + «NN%» +
  кнопка «100%» + health/power мелким текстом справа.

Кнопка «Fans + Power» переносится в сайдбар (раздел Fans) — на Dashboard её нет.

### 2.2 Fans

Содержимое текущего `fans-window.slint` (включая fan-curve-editor) переносится
в раздел без изменения контрактов (те же свойства/колбэки). При нехватке
высоты — внутренний `ScrollView`.

### 2.3 Hardware

Все оставшиеся поверхности железа — компактными строками (лейбл + контрол +
статус), из текущего `extra-window.slint`:

- Hotkeys M1–M5 (ComboBox-строки; draft/unavailable статус сохраняется);
- Aura Effect — disabled-строка (яркость клавиатуры остаётся на Dashboard);
- Panel Overdrive — `RequestToggleRow`;
- Boot sound — read-only `ToggleRow` (disabled);
- Power/CPU — iGPU memory (ComboBox), Hibernate after, P-cores, E-cores
  (SpinBox-строки).

Заголовки групп внутри раздела: «Keyboard / Aura», «Platform», «Power / CPU»,
«Hotkeys». Честные disabled-статусы сохраняются полностью.

### 2.4 Settings

- Theme (Dark/Light) — `RequestToggleRow` или ComboBox;
- Autostart, Start minimized, Remember window position — `RequestToggleRow`
  (как в текущем preferences-window);
- Close action — ComboBox (Hide to tray / Quit);
- Diagnostics — ряд из трёх `ActionButton`: Refresh / Copy / Export;
- About — версия + mock-profile (перенос текущего футера).

### 2.5 Удаляемое из UI

- `updates-window` — Slint-компонент и его колбэки удаляются; Rust-глейм
  updates-окна удаляется вместе с поверхностью. Typed-провайдеры updates в
  `orbis-providers` сохраняются (не UI-ответственность).
- `automation-window` — Slint-компонент и его UI-колбэки удаляются;
  Rust-слой automation (worker shadow/recovery) остаётся без изменений.
- `More`-меню, отдельные окна Preferences/Diagnostics — исчезают.
- `preview-dialog-window` — остаётся (используется ui-review сценариями).

## 3. Титлбар и frameless

- `Window { no-frame: true; background: transparent; }`, корневой `Rectangle`
  radius 10px + `clip: true` — скругление на всех углах.
- Титлбар 40px: слева название раздела (11px semibold, text-secondary),
  справа кнопки 28×28 с SVG-глифами из §5 (`window-minimize`, `window-close`;
  close — hover error-фон). Drag: вся площадь титлбара кроме кнопок.
- Drag реализуется через slint feature `unstable-winit-030`:
  `window.with_winit_window(|w| w.drag_window())` по нажатию в титлбаре
  (winit 0.30 `drag_window`, compositor ведёт окно сам). Фолбэк при
  недоступности — ручной перенос через `set_position` (не планируется,
  только если drag_window не сработает на целевом KDE Wayland).
- Close: колбэк в Rust → существующая логика close-action (hide-to-tray при
  живом хосте / quit) из #121. Minimize: `window.minimized = true` (проверяется
  на этапе плана; если в 1.13 недоступно — кнопка свернуть не добавляется).
- Позиция окна: remember-position продолжает работать с новым окном.

## 4. Тема Catppuccin

Маппинг токенов `Palette` (dark.slint / light.slint заменяют значения):

| Токен | Mocha (dark) | Latte (light) |
|---|---|---|
| window-background | #1E1E2E base | #EFF1F5 base |
| titlebar-background | #181825 mantle | #E6E9EF mantle |
| surface-default | #313244 surface0 | #CCD0DA surface0 |
| surface-hover | #45475A surface1 | #BCC0CC |
| surface-pressed | #181825 mantle | #DCE0E8 |
| surface-selected | #414356 | #C6CEE0 |
| surface-disabled | #262637 | #E0E3EA |
| border-default | #45475A surface1 | #9CA0B0 |
| border-strong | #585B70 surface2 | #7C7F93 |
| text-primary | #CDD6F4 text | #4C4F69 text |
| text-secondary | #A6ADC8 subtext0 | #6C6F85 subtext0 |
| text-disabled | #6C7086 overlay0 | #9CA0B0 |
| text-on-accent | #11111B crust | #EFF1F5 base |
| accent-default | #89B4FA blue | #1E66F5 blue |
| warning | #F9E2AF yellow | #DF8E1D yellow |
| error | #F38BA8 red | #D20F39 red |
| success | #A6E3A1 green | #40A02B green |
| silent | #A6E3A1 green | #40A02B |
| balanced | #89B4FA blue | #1E66F5 |
| turbo | #F38BA8 red | #D20F39 |
| eco | #A6E3A1 | #40A02B |
| standard | #89B4FA | #1E66F5 |
| ultimate | #FAB387 peach | #FE640B peach |
| optimized | #94E2D5 teal | #179299 teal |

`ThemeBridge` (std-widgets color-scheme) сохраняется. Selected-сегмент
заливается акцентом режима, текст — text-on-accent (как сейчас).

## 5. Иконки

`ui/assets/icons/*.svg` (монохром, stroke 1.5, 16×16 viewBox): dashboard, fan,
cpu, gear, window-minimize, window-close. Подключение через `image: @image-url`
+ `colorize` (Slint SVG требует cargo feature `svg` у slint — добавляется).
Иконки используются в сайдбаре и титлбаре; нигде больше.

## 6. Контракты и проверки

- `check-ui-contract.py`: порог главного окна меняется с ≤500×600 на
  ≤800×640 (и ≥400×400 сохраняется); маркеры main-window обновляются под
  новую структуру (display-mode-requested, keyboard-brightness-requested,
  preferences-маркеры переезжают в settings-секцию, diagnostics-маркеры — в
  settings-секцию). Инварианты (честные состояния, отсутствие fake-success
  фраз) не ослабляются.
- `main_tests.rs`: source-контракты (queued/pending, Optimized-инвариант)
  адаптируются под новую разметку без ослабления.
- ui-review: сценарии `--ui-state default/pending/disabled` +
  новый параметр `--ui-section dashboard|fans|hardware|settings` для
  скриншотов каждого раздела.
- Обязательные проверки после реализации: `cargo check/test/clippy -p
  orbis-ui --all-targets --locked`, `verify-static`, скриншоты всех секций
  обеих тем (минимум Mocha), `git diff --check`.

## 7. Вне объёма

- Никаких новых функций/мутаций; все safety-gates (#104/#105/#109/#116,
  product writes, unattended automation) остаются как есть.
- Worker/controller/session-клиенты не меняются (кроме удаления мёртвого
  UI-глея updates/automation-окон, если он есть).
- Resizable-окно, кастомные анимации, тёмный/светлый автопереключатель по
  системе — не входят.

## 8. Риски

- `unstable-winit-030` — нестабильная фича slint; риск минимален (winit 0.30
  зафиксирован в Cargo.lock), фолбэк описан в §3.
- SVG-иконки требуют cargo feature `svg`; если рендер SVG окажется некорректным
  в software-renderer скриншотов — фолбэк: глифы текстом/прямоугольниками для
  титлбара, векторные иконки только в сайдбаре (решение на этапе плана).
- Fluent-виджеты (ComboBox/SpinBox) под Catppuccin: ThemeBridge синхронизирует
  color-scheme; точечная доводка возможна после скриншотов.
- Высота Fans-редактора в 600px: при нехватке — внутренний ScrollView (§2.2).
