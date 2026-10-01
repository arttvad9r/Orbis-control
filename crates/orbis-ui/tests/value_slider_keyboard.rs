use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::platform::WindowEvent;

slint::slint! {
    import { ValueSlider } from "../../ui/widgets.slint";

    export component Harness inherits Window {
        width: 400px;
        height: 100px;
        in-out property <int> value: 100;
        in property <bool> disabled: false;
        out property <bool> focused: slider.has-keyboard-focus;
        callback changed(float);
        slider := ValueSlider {
            width: 400px;
            min: 40;
            max: 100;
            value: root.value;
            disabled: root.disabled;
            changed(v) => { root.changed(v); }
        }
    }
}

fn press(harness: &Harness, key: slint::platform::Key) {
    let text: slint::SharedString = key.into();
    let window = harness.window();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
}

fn setup(disabled: bool) -> (Harness, Rc<RefCell<Vec<f32>>>) {
    let harness = Harness::new().unwrap();
    harness.set_disabled(disabled);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    harness.on_changed(move |v| sink.borrow_mut().push(v));
    harness.show().unwrap();
    // Focus the slider the way a click would.
    let window = harness.window();
    window.set_size(slint::LogicalSize::new(400.0, 100.0));
    window.dispatch_event(WindowEvent::WindowActiveChanged(true));
    window.dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(200.0, 50.0),
    });
    window.dispatch_event(WindowEvent::PointerPressed {
        position: slint::LogicalPosition::new(200.0, 50.0),
        button: slint::platform::PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position: slint::LogicalPosition::new(200.0, 50.0),
        button: slint::platform::PointerEventButton::Left,
    });
    assert!(
        disabled || harness.get_focused(),
        "a click must focus the slider"
    );
    seen.borrow_mut().clear();
    (harness, seen)
}

fn arrow_keys_edit_locally_and_commit_once_after_the_pause() {
    let (harness, seen) = setup(false);
    harness.set_value(80);
    press(&harness, slint::platform::Key::LeftArrow);
    press(&harness, slint::platform::Key::LeftArrow);
    press(&harness, slint::platform::Key::LeftArrow);
    assert!(
        seen.borrow().is_empty(),
        "no write while the user is still typing"
    );
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(700));
    assert_eq!(*seen.borrow(), vec![77.0]);
}

fn home_end_and_enter_reach_the_range_limits() {
    let (harness, seen) = setup(false);
    harness.set_value(80);
    press(&harness, slint::platform::Key::End);
    press(&harness, slint::platform::Key::Return);
    assert_eq!(*seen.borrow(), vec![100.0]);
    press(&harness, slint::platform::Key::Home);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(700));
    assert_eq!(*seen.borrow(), vec![100.0, 40.0]);
}

fn disabled_slider_ignores_the_keyboard() {
    let (harness, seen) = setup(true);
    harness.set_value(80);
    press(&harness, slint::platform::Key::LeftArrow);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(700));
    assert!(seen.borrow().is_empty());
}

#[test]
fn keyboard_editing() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    arrow_keys_edit_locally_and_commit_once_after_the_pause();
    home_end_and_enter_reach_the_range_limits();
    disabled_slider_ignores_the_keyboard();
}
