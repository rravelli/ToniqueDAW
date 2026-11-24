//! The clip selections of the undo history. The engine keeps the edits'
//! history; this keeps, in step with each of its steps, the selection
//! before it when the step moved the selection (a duplicate selects the
//! copies): undoing the step brings it back, redoing the one it left.

use crate::session::selection::ClipSelection;

#[derive(Default)]
pub(crate) struct History {
    /// For each step to undo, oldest first.
    undo: Vec<Option<ClipSelection>>,
    /// The same for the undone steps, to redo.
    redo: Vec<Option<ClipSelection>>,
    /// The steps the engine had recorded when the open batch began.
    pub(crate) batch_start: Option<u64>,
}

impl History {
    /// The engine recorded a step: in step with it, no selection yet. It
    /// forgets its oldest steps past `depth`, the engine's.
    pub(crate) fn recorded(&mut self, depth: usize) {
        self.undo.push(None);
        self.redo.clear();
        let excess = self.undo.len().saturating_sub(depth);
        self.undo.drain(..excess);
    }

    /// The last step moved the selection from `before`.
    pub(crate) fn moved_selection(&mut self, before: ClipSelection) {
        if let Some(last) = self.undo.last_mut() {
            *last = Some(before);
        }
    }

    /// The engine undid a step: `selection` back to what it was before it.
    pub(crate) fn undone(&mut self, selection: &mut ClipSelection) {
        if let Some(entry) = self.undo.pop() {
            let after = entry.map(|before| std::mem::replace(selection, before));
            self.redo.push(after);
        }
    }

    /// The engine redid a step: `selection` to what it left.
    pub(crate) fn redone(&mut self, selection: &mut ClipSelection) {
        if let Some(entry) = self.redo.pop() {
            let before = entry.map(|after| std::mem::replace(selection, after));
            self.undo.push(before);
        }
    }

    /// Forget the selections, with the engine's history.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::edit::ClipId;

    pub(crate) fn selecting(ids: &[u64]) -> ClipSelection {
        ClipSelection {
            clips: ids.iter().copied().map(ClipId).collect(),
            bounds: None,
        }
    }

    #[test]
    pub(crate) fn undoing_and_redoing_swap_the_selection_a_step_moved() {
        let mut history = History::default();
        history.recorded(10);
        history.moved_selection(selecting(&[1]));
        let mut selection = selecting(&[2]);
        history.undone(&mut selection);
        assert_eq!(selection, selecting(&[1]));
        history.redone(&mut selection);
        assert_eq!(selection, selecting(&[2]));
    }

    #[test]
    pub(crate) fn steps_past_the_engines_depth_are_forgotten() {
        let mut history = History::default();
        for _ in 0..5 {
            history.recorded(3);
        }
        assert_eq!(history.undo.len(), 3);
    }
}
