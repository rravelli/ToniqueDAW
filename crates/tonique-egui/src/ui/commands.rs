//! One path for commands: shortcuts, menus and buttons push [`Action`]s;
//! the part of the UI owning what an action acts on takes it (the effects
//! panel, then the timeline), and the app runs the rest at the end of the
//! frame.

use crate::config::keymap::Action;

/// This frame's actions, in the order asked for.
#[derive(Default)]
pub struct Commands {
    actions: Vec<Action>,
}

impl Commands {
    pub fn push(&mut self, action: Action) {
        self.actions.push(action);
    }

    /// Remove the actions `accept` wants, in order.
    pub fn take(&mut self, accept: impl Fn(Action) -> bool) -> Vec<Action> {
        let (taken, rest) = self.actions.drain(..).partition(|a| accept(*a));
        self.actions = rest;
        taken
    }

    /// Remove every action.
    pub fn take_all(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_leaves_the_rest_in_order() {
        let mut commands = Commands::default();
        for action in [
            Action::Undo,
            Action::Delete,
            Action::Redo,
            Action::SelectAll,
        ] {
            commands.push(action);
        }
        assert_eq!(
            commands.take(Action::is_timeline),
            [Action::Delete, Action::SelectAll]
        );
        assert_eq!(commands.take_all(), [Action::Undo, Action::Redo]);
        assert!(commands.take_all().is_empty());
    }
}
