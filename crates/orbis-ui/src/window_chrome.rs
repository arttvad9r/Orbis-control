//! Frameless window chrome helpers.

use slint::platform::{PointerEventButton, WindowEvent};

/// Call right after handing an interactive move to the compositor
/// (`winit::Window::drag_window` / `drag_resize_window`).
///
/// The compositor consumes the pointer release that ends the move, so Slint
/// never sees it and keeps the pointer grabbed by the title bar: every later
/// click would land on the drag surface and start another move, leaving the
/// rest of the window unclickable. Synthesizing the release (deferred, since
/// this runs inside the press handler) and an exit ends that grab.
pub fn end_system_move<C: slint::ComponentHandle + 'static>(component: &C) {
    let weak = component.as_weak();
    let release = move || {
        let Some(component) = weak.upgrade() else {
            return;
        };
        let window = component.window();
        window.dispatch_event(WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(-1.0, -1.0),
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerExited);
    };
    slint::Timer::single_shot(std::time::Duration::ZERO, release);
}
