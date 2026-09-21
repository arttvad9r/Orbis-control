# G-Helper Parity Matrix (D-012)

Решение D-012 (locked, 2026-09-22, пользователь): Orbis реализует ВЕСЬ функционал G-Helper.
Референс: G-Helper Windows (официальный README) + g-helper-linux v1.0.92 (доказательство
реализуемости на Linux). «Без оправданий» = каждая фича-группа реализуется в коде;
доступность на конкретной машине определяется capability-discovery (честный
Unsupported с причиной), а не исключением из объёма.

Статусы: YES = есть и работает; PART = частично; NO = отсутствует; WAVE = план.
Волны P1..P6 — пост-v0.1 дорожная карта (старт после закрытия карты R релиза v0.1).

## Матрица

| # | Функция G-Helper | Windows | g-helper-linux | Orbis | План |
|---|---|---|---|---|---|
| 1 | Профили Silent/Balanced/Turbo | + | + | YES | — |
| 2 | Кривые вентиляторов (по fan, per-mode) | + | 8-point | YES (B) | — |
| 3 | Power limits PL1/PL2 per-mode | + | + | C (в работе) | — |
| 4 | CPU boost / turbo | + | + + EPP | YES (AC-040) | EPP-полнота → P1 |
| 5 | GPU modes Eco/Std/Optimized/Ultimate + MUX | + | + | D (в очереди) | — |
| 6 | Dynamic Boost | + | + | C | — |
| 7 | GPU temp limit | + | + | PART (LACT-telemetry) | P2 |
| 8 | NVIDIA core/mem clock offsets, GPU power (OC/UV) | + | + | NO | P2 |
| 9 | AMD iGPU/dGPU control | + | + | PART | P2 |
| 10 | AMD Curve Optimizer undervolt (ryzen_smu) | — | + | NO (есть ryzenadj-diagnostics) | P3 |
| 11 | Intel MSR undervolt | + | + | NO | P3 |
| 12 | Battery charge limit | + | 40–100% | YES | — |
| 13 | Refresh rate + Panel OD | + | + | YES (refresh read + OD) | — |
| 14 | MiniLED multi-zone | + | + | NO | P5 |
| 15 | Display brightness/gamma multi-backend | + | + | PART (Wayland outputs) | P5 |
| 16 | Flicker-free dimming / Visual modes | + | + | NO | P5 |
| 17 | Keyboard backlight (brightness) | + | + | YES | — |
| 18 | Aura RGB static | + | + | YES | — |
| 19 | Aura анимации/per-key | + | + | NO (static only) | P4 |
| 20 | Anime Matrix / Slash (GIF, clock, visualizer) | + | + | NO | P4 |
| 21 | XG Mobile dock | + | + | NO | P4 |
| 22 | FnLock + кастомные хоткеи | + | + | NO | P4 |
| 23 | NumberPad | + | + | NO | P4 |
| 24 | Мыши ASUS + Logitech HID++ | + | + | NO | P4 |
| 25 | Audio DSP (EQ, noise suppression, reverb) | — | + | NO | P6 |
| 26 | Hardware overlay (FPS/temps OSD) | + | + | NO | P6 |
| 27 | System monitor + tray stats | + | + | YES (dashboard/tray) | — |
| 28 | Автоматика: профиль по питанию | + | + | NO (SPEC-бан снят D-012) | P1 |
| 29 | Автоматика: Optimized GPU (Eco on battery) | + | + | NO | P1 |
| 30 | Автоматика: auto refresh rate | + | + | NO | P1 |
| 31 | Автоматика: kbd backlight timeout | + | + | NO | P1 |
| 32 | Update check + changelog | — | + | NO (self-update убран намеренно) | P6 (check-only, установка через pacman) |
| 33 | BIOS/driver updates | + | — | NO | P6 (fwupd/vendor-путь, честный gating) |
| 34 | ROG Ally handheld mode | + | + | NO | P6 (capability-gated) |
| 35 | Auto-start | + | + | YES (systemd user) | — |
| 36 | Per-app profiles / per-game | — | — | NO (SPEC-бан) | вне паритета: G-Helper этого не умеет |

## Правила волн

1. Волны идут строго после R (релиз v0.1): чистый baseline для больших расширений.
2. Каждая волна = implementation-карта + независимая QA-карта, D-011 (G-Helper-linux
   как первичный референс) действует на каждую.
3. Инварианты неизменны: GUI без root; мутации только через Hardware1; capability
   по evidence; Desired/Observed/Pending раздельны; честный Unsupported.
4. Автоматика (P1) НЕ превращает sessiond в mutation-deputy: движок правил живёт в
   user-компоненте и вызывает те же авторизованные пути Hardware1 (polkit).
5. Фича без железа на этой машине (AnimeMatrix, XGM, Ally, MiniLED) реализуется
   с провайдерами + честным gating; проверка на моках + по возможности на живом
   устройстве. Отсутствие железа не отменяет реализацию (D-012), но живую валидацию
   такой фичи помечает как не-проверено-на-железе.
6. Windows-специфика (Windows Power Mode) отображается на Linux-эквивалент (EPP).

## Волны

- P1 Автоматика: правила по питанию (профиль, GPU Optimized, refresh, backlight timeout) + UI правил.
- P2 GPU tuning: core/mem offsets, GPU power, temp limit (NVIDIA через nvidia-ml/LACT-путь, AMD через sysfs/UMD), iGPU-донор.
- P3 Undervolting: AMD Curve Optimizer (ryzen_smu, capability-hidden), Intel MSR (platt/MSR, capability-hidden).
- P4 LED и периферия: Aura-анимации/per-key, AnimeMatrix/Slash, XG Mobile, FnLock/хоткеи, NumberPad, мыши (HID++/HID raw).
- P5 Display extras: MiniLED зоны, brightness/gamma multi-backend, flicker-free/visual modes.
- P6 Misc: audio DSP (UCM/Deep-путь — исследовать по референсу), OSD overlay, update-check + changelog, Ally handheld, BIOS/fwupd.
