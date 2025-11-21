use crate::core::state::{ToniqueProjectState, action::BatchAction};

impl ToniqueProjectState {
    /// Create a batch of actions. All actions made from this point are not applied but saved to a buffer.
    /// Use `commit_batch` to apply them.
    pub fn begin_batch(&mut self) {
        self.batching = true;
    }
    /// Apply changes saved in the batch buffer. New actions are no longer saved in the buffer.
    pub fn commit_batch(&mut self) {
        self.batching = false;
        if self.batch_buffer.len() > 0 {
            let batch = std::mem::take(&mut self.batch_buffer);
            let action = BatchAction::new(batch);
            self.apply_action(Box::new(action));
        }
        self.batch_buffer.clear();
    }
    /// Undo last action. Does nothing if there is no action.
    pub fn undo(&mut self) {
        if let Some(mut action) = self.undo_stack.pop() {
            if cfg!(debug_assertions) {
                println!("Undoing {}", action.name());
            }
            action.undo(self);
            self.redo_stack.push(action);
        }
    }
    /// Redo last action. Does nothing if there is no action.
    pub fn redo(&mut self) {
        if let Some(mut action) = self.redo_stack.pop() {
            if cfg!(debug_assertions) {
                println!("Redoing {}", action.name());
            }
            action.apply(self);
            self.undo_stack.push(action);
        }
    }
    /// Whether there is still actions to undo
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    /// Whether there is still actions to redo
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
