# Orbis Control — Interaction Contract (v0.1 iteration)
#
# Source: SPEC.md + ui/audited pages. Desktop app, Wayland-first, Russian UI strings.
# Primary window: 980x680 (min), 1200x800 (comfort). Left nav, single window.
# Owners: WS-CORE = core/application+backend flow; WS-UI = audited Slint UI; WS-QA = independent QA.
# Workstreams are declared in acceptance-contract.yaml `workstreams` (same ids).

app:
  name: Orbis Control
  platform: DESKTOP
  window_sizes: [[980, 680], [1200, 800]]
  themes: [dark, light]
  locale: ru-RU

workstreams_note: owners WS-CORE/WS-UI/WS-QA declared in acceptance-contract.yaml

interactions:
  - |
    | control | action | observable | implementation | runtime_owner | qa |
    | --- | --- | --- | --- | --- | --- |
    | NAV-DASHBOARD | open Dashboard page | Loading переходит в реальные CPU/GPU/battery/AC состояния без фикстур | WS-CORE | WS-CORE | SC-LAUNCH и SC-BACKEND-GONE покрывают запуск и honest-состояния |
    | NAV-PERFORMANCE | open Performance page | Текущий и доступные профили из backend; power limits видны при capability | WS-CORE | WS-CORE | SC-PROFILE-SET и SC-POWER-VIEW |
    | NAV-COOLING | open Cooling page | CPU и GPU fan отдельно; кривые читаются; Dirty помечен | WS-UI | WS-CORE | SC-FAN-CPU и SC-FAN-GPU |
    | NAV-GRAPHICS | open Graphics page | GPU mode: current vs queued vs reboot-required разведены | WS-CORE | WS-CORE | SC-GPU-MODE и SC-GPU-PARTIAL |
    | NAV-SYSTEM | open System page | Идентификация устройства и диагностика без краша | WS-UI | WS-UI | визуальная проверка human visual review, состояния читаемы |
    | NAV-PREFERENCES | open Preferences page | Autostart/tray/тема работают из установленных путей | WS-UI | WS-UI | визуальная проверка human visual review |
    | PROFILE-SELECT | select supported profile | Применение через Hardware1; read-back подтверждает; повторный выбор активного безопасен | WS-CORE | WS-CORE | SC-PROFILE-SET и SC-REGRESSION |
    | FAN-CURVE-EDIT | edit curve point | До Apply железо не меняется; Dirty виден; невалидная точка отклоняется | WS-CORE | WS-CORE | SC-FAN-CPU, SC-FAN-GPU, SC-FAN-RESET |
    | FAN-APPLY | click Apply | Typed path; read-back; mismatch показан как ошибка | WS-CORE | WS-CORE | SC-FAN-CPU и SC-FAN-GPU |
    | FAN-DISCARD | switch fan/profile with dirty | Dirty сбрасывается с предупреждением; железо не меняется | WS-UI | WS-CORE | SC-FAN-CPU покрывает dirty-семантику |
    | POWER-LIMIT-SET | set SPL/SPPT/FPPT draft | Только значения в authoritative диапазоне; draft отделён от observed; pending блокирует повтор | WS-CORE | WS-CORE | SC-POWER-VIEW и SC-POWER-APPLY |
    | POWER-LIMIT-APPLY | click Apply limits | Вне диапазона — отказ; успех подтверждён read-back; polkit-отказ и неизвестный исход видны | WS-CORE | WS-CORE | SC-POWER-APPLY |
    | DYNAMIC-BOOST-SET | set Dynamic Boost | Typed write с read-back; без capability контрол скрыт/Unsupported | WS-CORE | WS-CORE | SC-BOOST |
    | GPU-MODE-SELECT | select GPU mode | Ultimate не Applied до перехода; reboot-required помечен; no-op honest | WS-CORE | WS-CORE | SC-GPU-MODE |
    | UNSUPPORTED-CONTROL | observe unsupported/readonly capability | Нет dead-контролов; read-only объяснён, значения реальны | WS-CORE | WS-CORE | SC-UNSUPPORTED и SC-READONLY |
    | THEME-SWITCH | toggle dark/light | Обе темы читаемы на 980x680 и 1200x800 без overlap/clipping | WS-UI | WS-UI | SC-VISUAL-980 и SC-VISUAL-STATES |
    | PREFS-RESTORE | restart app after change | Hardware state не восстанавливается из preferences без evidence | WS-CORE | WS-CORE | SC-NO-RESTORE |
