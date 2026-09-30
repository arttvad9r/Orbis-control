//! Frameless title bar: window buttons must keep working after the bar hands
//! a move to the compositor (winit `drag_window` swallows the button release,
//! which used to leave Slint's pointer grab stuck on the drag surface).

use std::cell::Cell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::platform::{PointerEventButton, WindowEvent};

slint::slint! {
    import { TitleBar } from "../../ui/components/title-bar.slint";

    export component Harness inherits Window {
        width: 800px;
        height: 48px;
        callback minimize();
        callback maximize();
        callback close();
        callback drag();
        TitleBar {
            width: 800px;
            minimize-requested => { root.minimize(); }
            maximize-requested => { root.maximize(); }
            close-requested => { root.close(); }
            drag-started => { root.drag(); }
        }
    }
}

fn pos(x: f32, y: f32) -> slint::LogicalPosition {
    slint::LogicalPosition::new(x, y)
}

fn click(window: &slint::Window, x: f32, y: f32) {
    window.dispatch_event(WindowEvent::PointerMoved {
        position: pos(x, y),
    });
    window.dispatch_event(WindowEvent::PointerPressed {
        position: pos(x, y),
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position: pos(x, y),
        button: PointerEventButton::Left,
    });
}

#[test]
fn window_buttons_survive_a_system_move() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let harness = Harness::new().unwrap();
    let counts = Rc::new([
        Cell::new(0u32),
        Cell::new(0u32),
        Cell::new(0u32),
        Cell::new(0u32),
    ]);
    let c = counts.clone();
    harness.on_minimize(move || c[0].set(c[0].get() + 1));
    let c = counts.clone();
    harness.on_maximize(move || c[1].set(c[1].get() + 1));
    let c = counts.clone();
    harness.on_close(move || c[2].set(c[2].get() + 1));
    let weak = harness.as_weak();
    let c = counts.clone();
    harness.on_drag(move || {
        c[3].set(c[3].get() + 1);
        // What production does after handing the move to the compositor.
        if let Some(harness) = weak.upgrade() {
            orbis_ui::window_chrome::end_system_move(&harness);
        }
    });
    harness.show().unwrap();
    let window = harness.window();

    // A plain click on the bar is not a move.
    click(window, 400.0, 24.0);
    assert_eq!(counts[3].get(), 0, "a click must not start a window move");

    // Press and drag: the move starts once, and the release never arrives.
    window.dispatch_event(WindowEvent::PointerMoved {
        position: pos(400.0, 24.0),
    });
    window.dispatch_event(WindowEvent::PointerPressed {
        position: pos(400.0, 24.0),
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerMoved {
        position: pos(420.0, 30.0),
    });
    window.dispatch_event(WindowEvent::PointerMoved {
        position: pos(440.0, 30.0),
    });
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(20));
    assert_eq!(
        counts[3].get(),
        1,
        "dragging the bar starts exactly one move"
    );

    // Buttons (right edge: minimize, maximize, close — 50 px each).
    click(window, 800.0 - 125.0, 24.0);
    click(window, 800.0 - 75.0, 24.0);
    click(window, 800.0 - 25.0, 24.0);
    assert_eq!(counts[0].get(), 1, "minimize");
    assert_eq!(counts[1].get(), 1, "maximize");
    assert_eq!(counts[2].get(), 1, "close");

    // Double-click on the bar toggles maximize (desktop convention).
    click(window, 300.0, 24.0);
    click(window, 300.0, 24.0);
    assert_eq!(counts[1].get(), 2, "double-click maximizes");
}
