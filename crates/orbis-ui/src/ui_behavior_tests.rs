//! Behaviour of the real `AppWindow` on a headless Slint backend: which rows a
//! state shows, and which callbacks a click (accessibility default action)
//! reaches. These replace assertions on the `.slint` source text.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle, ElementQuery};

use super::*;

thread_local! {
    static BACKEND: Cell<bool> = const { Cell::new(false) };
}

/// A fresh window on this test thread's own testing backend (the Slint
/// context is per thread, so tests stay independent).
fn window_on(section: Section) -> AppWindow {
    BACKEND.with(|ready| {
        if !ready.get() {
            i_slint_backend_testing::init_no_event_loop();
            ready.set(true);
        }
    });
    let app = AppWindow::new().expect("headless AppWindow");
    // Tall enough that no row is clipped by a section's scroll area (the
    // element search skips clipped items, as a user could not see them).
    app.window()
        .set_size(slint::LogicalSize::new(1240.0, 4000.0));
    app.set_active_section(section);
    app
}

fn shows(app: &AppWindow, label: &str) -> bool {
    ElementHandle::find_by_accessible_label(app, label)
        .next()
        .is_some()
}

fn control(app: &AppWindow, label: &str, role: AccessibleRole) -> ElementHandle {
    ElementHandle::find_by_accessible_label(app, label)
        .find(|element| element.accessible_role() == Some(role))
        .unwrap_or_else(|| panic!("no {role:?} labelled {label:?}"))
}

#[test]
fn sidebar_opens_every_section() {
    let app = window_on(Section::Dashboard);
    for (label, section) in [
        ("Производительность", Section::Performance),
        ("Питание", Section::Power),
        ("Охлаждение", Section::Cooling),
        ("Графика", Section::Graphics),
        ("Подсветка", Section::Backlight),
        ("Экран", Section::Display),
        ("Система", Section::System),
        ("Настройки", Section::Settings),
        ("О программе", Section::About),
        ("Главная", Section::Dashboard),
    ] {
        control(&app, label, AccessibleRole::Tab).invoke_accessible_default_action();
        assert_eq!(app.get_active_section(), section, "nav item {label}");
    }
}

#[test]
fn power_state_lighting_card_follows_asusd_evidence_and_write_gate() {
    let app = window_on(Section::Backlight);
    assert!(
        !shows(&app, "Подсветка по состояниям"),
        "hidden without LedPower"
    );

    app.set_aura_power_ready(true);
    app.set_aura_power_awake(true);
    assert!(shows(&app, "Подсветка по состояниям"));
    let requests = Rc::new(RefCell::new(Vec::new()));
    let seen = requests.clone();
    app.on_aura_power_requested(move |which, on| seen.borrow_mut().push((which, on)));

    let sleep = control(&app, "Во сне", AccessibleRole::Switch);
    assert_eq!(
        sleep.accessible_enabled(),
        Some(false),
        "read-only until writable"
    );
    sleep.invoke_accessible_default_action();
    assert!(requests.borrow().is_empty());

    app.set_aura_control_ready(true);
    sleep.invoke_accessible_default_action();
    control(&app, "Во время работы", AccessibleRole::Switch).invoke_accessible_default_action();
    assert_eq!(*requests.borrow(), vec![(2, true), (1, false)]);
}

#[test]
fn aura_effects_offer_only_supported_modes_and_send_the_chosen_one() {
    let app = window_on(Section::Backlight);
    app.set_aura_state_ready(true);
    app.set_aura_control_ready(true);
    app.set_aura_static_supported(true);
    app.set_aura_breathe_supported(true);
    assert!(shows(&app, "Breathe"));
    assert!(!shows(&app, "Laser"), "unsupported mode is not offered");
    app.set_aura_laser_supported(true);
    assert!(shows(&app, "Laser"));

    let modes = Rc::new(RefCell::new(Vec::new()));
    let seen = modes.clone();
    app.on_aura_effect_requested(move |mode, _, _, _, _, _, _, _| seen.borrow_mut().push(mode));
    control(&app, "Breathe", AccessibleRole::Button).invoke_accessible_default_action();
    assert_eq!(*modes.borrow(), vec![1]);
}

#[test]
fn graphics_marks_the_queued_mode_and_the_pending_reboot() {
    let app = window_on(Section::Graphics);
    let mut state = controller::UiState::from_mock_profile("zephyrus-full");
    state.available_gpu_mask = 0b1111;
    state.gpu_queued = -1;
    state.gpu_reboot_required = false;
    app.set_ui_state(to_slint(&state));
    assert!(shows(&app, "Optimized"));
    assert!(!shows(&app, "После перезагрузки"));

    state.gpu_queued = 0;
    state.gpu_reboot_required = true;
    app.set_ui_state(to_slint(&state));
    assert_eq!(
        ElementHandle::find_by_accessible_label(&app, "После перезагрузки").count(),
        1,
        "only the queued Eco card is marked"
    );
    assert!(shows(
        &app,
        "В очереди: Eco · применится после перезагрузки"
    ));
}

#[test]
fn performance_limits_card_is_honest_and_cpu_boost_respects_write_gate() {
    let app = window_on(Section::Performance);
    app.set_ui_state(to_slint(&controller::UiState::production_initial()));
    assert!(
        shows(&app, "Лимиты мощности и температуры"),
        "card stays with a reason"
    );
    assert!(!shows(&app, "NVIDIA Dynamic Boost"));

    app.set_cpu_tuning_ready(true);
    app.set_cpu_boost_known(true);
    app.set_cpu_boost_writable(false);
    let requests = Rc::new(Cell::new(0));
    let seen = requests.clone();
    app.on_cpu_boost_requested(move |_| seen.set(seen.get() + 1));
    let boost = control(&app, "Турбо-буст", AccessibleRole::Switch);
    assert_eq!(boost.accessible_enabled(), Some(false));
    boost.invoke_accessible_default_action();
    assert_eq!(requests.get(), 0);
    app.set_cpu_boost_writable(true);
    boost.invoke_accessible_default_action();
    assert_eq!(requests.get(), 1);
}

#[test]
fn system_advanced_rows_appear_only_with_their_evidence() {
    let app = window_on(Section::System);
    app.set_auto_clamshell_state(ClamshellState::Unavailable);
    for label in [
        "Параметры ASUS",
        "Звук при включении",
        "Без энергосбережения PCIe",
        "Память iGPU",
        "Не засыпать при закрытой крышке",
    ] {
        assert!(!shows(&app, label), "{label} shown without evidence");
    }
    app.set_boot_sound_state_ready(true);
    assert!(shows(&app, "Параметры ASUS"));
    assert!(shows(&app, "Звук при включении"));
    assert!(!shows(&app, "Память iGPU"));
    app.set_igpu_memory_state_ready(true);
    app.set_aspm_state_ready(true);
    app.set_auto_clamshell_state(ClamshellState::Inactive);
    for label in [
        "Без энергосбережения PCIe",
        "Память iGPU",
        "Не засыпать при закрытой крышке",
    ] {
        assert!(shows(&app, label), "{label} missing with evidence");
    }
    // Rows that have no typed backend owner are never offered.
    for owner_less in [
        "Светодиоды состояния",
        "Гибернация через",
        "Активные ядра CPU",
        "КЛАВИШИ M1–M5",
    ] {
        assert!(
            ElementQuery::from_root(&app)
                .match_descendants()
                .match_predicate(move |e| {
                    e.accessible_label().is_some_and(|l| l.contains(owner_less))
                })
                .find_first()
                .is_none(),
            "no typed owner: {owner_less}"
        );
    }
}
