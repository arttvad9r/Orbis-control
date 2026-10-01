//! Behaviour of the real `AppWindow` on a headless Slint backend: which rows a
//! state shows, and which callbacks a click (accessibility default action)
//! reaches. These replace assertions on the `.slint` source text.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};

use super::*;

thread_local! {
    static BACKEND: Cell<bool> = const { Cell::new(false) };
}

/// A fresh window on this test thread's own testing backend (the Slint
/// context is per thread, so tests stay independent).
fn window_with(panel: Panel) -> AppWindow {
    BACKEND.with(|ready| {
        if !ready.get() {
            i_slint_backend_testing::init_no_event_loop();
            ready.set(true);
        }
    });
    let app = AppWindow::new().expect("headless AppWindow");
    app.set_panel(panel);
    // Tall enough that no row is clipped by a panel's scroll area (the
    // element search skips clipped items, as a user could not see them).
    app.window()
        .set_size(slint::LogicalSize::new(841.0, 4000.0));
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
fn footer_buttons_toggle_the_side_panels() {
    let app = window_with(Panel::None);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    app.on_panel_changed(move |panel| sink.borrow_mut().push(panel));

    control(&app, "Вентиляторы", AccessibleRole::Button).invoke_accessible_default_action();
    assert_eq!(app.get_panel(), Panel::Fans);
    control(&app, "Дополнительно", AccessibleRole::Button).invoke_accessible_default_action();
    assert_eq!(app.get_panel(), Panel::Extra);
    control(&app, "Дополнительно", AccessibleRole::Button).invoke_accessible_default_action();
    assert_eq!(
        app.get_panel(),
        Panel::None,
        "second press closes the panel"
    );
    assert_eq!(*seen.borrow(), vec![Panel::Fans, Panel::Extra, Panel::None]);
}

#[test]
fn power_state_lighting_follows_asusd_evidence_and_write_gate() {
    let app = window_with(Panel::Extra);
    assert!(!shows(&app, "Во сне"), "hidden without LedPower");

    app.set_aura_power_ready(true);
    app.set_aura_power_awake(true);
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
fn keyboard_row_offers_only_reported_controls() {
    let app = window_with(Panel::None);
    assert!(!shows(&app, "Эффект подсветки"));
    assert!(!shows(&app, "Яркость подсветки 2"));

    app.set_keyboard_state_ready(true);
    app.set_keyboard_control_ready(true);
    app.set_keyboard_brightness(1);
    assert!(shows(&app, "Яркость подсветки 2"));
    assert!(
        !shows(&app, "Эффект подсветки"),
        "no Aura evidence, no effects"
    );
    let levels = Rc::new(RefCell::new(Vec::new()));
    let seen = levels.clone();
    app.on_keyboard_brightness_requested(move |level| seen.borrow_mut().push(level));
    control(&app, "Подсветка выключена", AccessibleRole::Button).invoke_accessible_default_action();
    assert_eq!(*levels.borrow(), vec![0]);

    app.set_aura_state_ready(true);
    app.set_keyboard_effect(1);
    let effect = control(&app, "Эффект подсветки", AccessibleRole::Combobox);
    assert_eq!(effect.accessible_value().as_deref(), Some("Дыхание"));
}

#[test]
fn graphics_hides_unreported_modes_and_names_the_queued_one() {
    let app = window_with(Panel::None);
    let mut state = controller::UiState::from_mock_profile("zephyrus-full");
    state.available_gpu_mask = 0b0011;
    state.gpu_queued = -1;
    state.gpu_reboot_required = false;
    app.set_ui_state(to_slint(&state));
    assert!(shows(&app, "Eco"));
    assert!(!shows(&app, "Ultimate"), "not reported, not offered");
    assert!(!shows(&app, "Optimized"));

    state.available_gpu_mask = 0b1111;
    state.gpu_queued = 0;
    state.gpu_reboot_required = true;
    app.set_ui_state(to_slint(&state));
    assert!(shows(&app, "Optimized"));
    assert!(shows(&app, "Eco применится после перезагрузки"));
}

#[test]
fn power_limits_stay_hidden_until_reported_and_cpu_boost_respects_write_gate() {
    let app = window_with(Panel::Fans);
    app.set_ui_state(to_slint(&controller::UiState::production_initial()));
    assert!(!shows(&app, "Лимиты мощности"));
    assert!(!shows(&app, "Dynamic Boost"));

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
fn fan_apply_sends_the_factory_reset_only_after_it_was_chosen() {
    let app = window_with(Panel::Fans);
    let mut state = controller::UiState::from_mock_profile("zephyrus-full");
    state.fan_curve_state = controller::FanCurveHwState::Ready;
    state.fan_curve_writable = true;
    state.fan_curve_dirty = false;
    app.set_ui_state(to_slint(&state));
    let applies = Rc::new(RefCell::new(Vec::new()));
    let seen = applies.clone();
    app.on_fan_apply_clicked(move |reset| seen.borrow_mut().push(reset));

    let apply = control(&app, "Применить", AccessibleRole::Button);
    assert_eq!(apply.accessible_enabled(), Some(false), "nothing to apply");
    control(&app, "Заводская", AccessibleRole::Button).invoke_accessible_default_action();
    apply.invoke_accessible_default_action();
    assert_eq!(*applies.borrow(), vec![true]);
}

#[test]
fn system_rows_appear_only_with_their_evidence() {
    let app = window_with(Panel::Extra);
    app.set_auto_clamshell_state(ClamshellState::Unavailable);
    for label in [
        "Звук при включении",
        "Без энергосбережения PCIe (до перезагрузки)",
        "Память iGPU",
        "Не засыпать при закрытой крышке",
    ] {
        assert!(!shows(&app, label), "{label} shown without evidence");
    }
    app.set_boot_sound_state_ready(true);
    assert!(shows(&app, "Звук при включении"));
    assert!(!shows(&app, "Память iGPU"));
    app.set_igpu_memory_state_ready(true);
    app.set_aspm_state_ready(true);
    app.set_auto_clamshell_state(ClamshellState::Inactive);
    for label in [
        "Без энергосбережения PCIe (до перезагрузки)",
        "Память iGPU",
        "Не засыпать при закрытой крышке",
    ] {
        assert!(shows(&app, label), "{label} missing with evidence");
    }
}
