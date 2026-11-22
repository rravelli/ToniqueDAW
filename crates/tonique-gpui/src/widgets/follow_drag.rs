//! A drag that follows the pointer anywhere in the window, over other
//! views and dialogs too: an element only hears the pointer while it's
//! over it, so a view dragging something listens to the whole window,
//! before any element, until the button is let go.

use gpui::{
    Context, DispatchPhase, IntoElement, MouseMoveEvent, MouseUpEvent, Styled, WeakEntity, canvas,
};

/// Calls `on_move` on `view` at each pointer move and `on_up` when the
/// button is let go, then redraws it. A child of the view while it drags.
pub fn follow_drag<V: 'static>(
    view: WeakEntity<V>,
    on_move: fn(&mut V, &MouseMoveEvent, &mut Context<V>),
    on_up: fn(&mut V, &MouseUpEvent, &mut Context<V>),
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let released = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, context| {
                if phase == DispatchPhase::Capture {
                    let _ = view.update(context, |view, context| {
                        on_move(view, event, context);
                        context.notify();
                    });
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, context| {
                if phase == DispatchPhase::Capture {
                    let _ = released.update(context, |view, context| {
                        on_up(view, event, context);
                        context.notify();
                    });
                }
            });
        },
    )
    .absolute()
    .size_0()
}
