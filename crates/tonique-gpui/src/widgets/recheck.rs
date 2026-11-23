//! Rendering a view again a little later, while work it shows goes on in
//! the background (a file decoding, folders being read): nothing tells the
//! view when that work moves, so it looks again.

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{Context, Task};

/// How long a view waits before looking again.
const INTERVAL: Duration = Duration::from_millis(100);

/// A view's next look, once asked for.
#[derive(Default)]
pub struct Recheck {
    waiting: Rc<Cell<bool>>,
    _task: Option<Task<()>>,
}

impl Recheck {
    /// Renders the view again soon, unless it already will.
    pub fn soon<V: 'static>(&mut self, context: &mut Context<V>) {
        if self.waiting.replace(true) {
            return;
        }
        let waiting = self.waiting.clone();
        self._task = Some(context.spawn(async move |this, context| {
            context.background_executor().timer(INTERVAL).await;
            waiting.set(false);
            this.update(context, |_, context| context.notify()).ok();
        }));
    }
}
