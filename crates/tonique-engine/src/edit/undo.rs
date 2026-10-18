use super::commands::EditCommand;
use super::{Edit, EditError};
use crate::param::ParamId;

/// What the engine must do after an edit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    pub rebuild: bool,
    /// Automation curves to swap into running automation nodes.
    pub curves: Vec<ParamId>,
}

impl Effects {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn rebuild() -> Self {
        Self {
            rebuild: true,
            curves: Vec::new(),
        }
    }

    pub fn curve(id: ParamId) -> Self {
        Self {
            rebuild: false,
            curves: vec![id],
        }
    }

    pub fn merge(&mut self, other: Effects) {
        self.rebuild |= other.rebuild;
        for c in other.curves {
            if !self.curves.contains(&c) {
                self.curves.push(c);
            }
        }
    }
}

pub struct Transaction {
    pub label: &'static str,
    commands: Vec<Box<dyn EditCommand>>,
}

/// Linear undo history with explicit transactions for compound gestures
/// (a drag is many `MoveClip`s but one undo step).
pub struct UndoManager {
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
    open: Option<Transaction>,
    max_depth: usize,
    /// Steps recorded so far, ever: tells callers when one was.
    recorded: u64,
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new(200)
    }
}

impl UndoManager {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            open: None,
            max_depth,
            recorded: 0,
        }
    }

    pub fn perform(
        &mut self,
        edit: &mut Edit,
        mut cmd: Box<dyn EditCommand>,
    ) -> Result<Effects, EditError> {
        let effects = cmd.apply(edit)?;
        self.redo_stack.clear();
        match &mut self.open {
            Some(t) => t.commands.push(cmd),
            None => self.push(Transaction {
                label: cmd.label(),
                commands: vec![cmd],
            }),
        }
        Ok(effects)
    }

    fn push(&mut self, t: Transaction) {
        self.recorded += 1;
        self.undo_stack.push(t);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    pub fn begin_transaction(&mut self, label: &'static str) {
        if self.open.is_none() {
            self.open = Some(Transaction {
                label,
                commands: Vec::new(),
            });
        }
    }

    pub fn commit_transaction(&mut self) {
        if let Some(t) = self.open.take()
            && !t.commands.is_empty()
        {
            self.push(t);
        }
    }

    /// Revert everything done since `begin_transaction` (e.g. Esc mid-drag).
    pub fn cancel_transaction(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let mut effects = Effects::none();
        if let Some(mut t) = self.open.take() {
            for c in t.commands.iter_mut().rev() {
                effects.merge(c.revert(edit)?);
            }
        }
        Ok(effects)
    }

    pub fn undo(&mut self, edit: &mut Edit) -> Result<Option<Effects>, EditError> {
        self.commit_transaction();
        let Some(mut t) = self.undo_stack.pop() else {
            return Ok(None);
        };
        let mut effects = Effects::none();
        for c in t.commands.iter_mut().rev() {
            effects.merge(c.revert(edit)?);
        }
        self.redo_stack.push(t);
        Ok(Some(effects))
    }

    pub fn redo(&mut self, edit: &mut Edit) -> Result<Option<Effects>, EditError> {
        let Some(mut t) = self.redo_stack.pop() else {
            return Ok(None);
        };
        let mut effects = Effects::none();
        for c in t.commands.iter_mut() {
            effects.merge(c.apply(edit)?);
        }
        self.undo_stack.push(t);
        Ok(Some(effects))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || self.open.as_ref().is_some_and(|t| !t.commands.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Forget every step, e.g. once a project is loaded.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.open = None;
    }

    /// How many steps have been recorded (not undone or redone: new ones),
    /// ever. It changes when an edit becomes a step of its own.
    pub fn steps_recorded(&self) -> u64 {
        self.recorded
    }

    /// Steps there are to undo.
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo_stack.last().map(|t| t.label)
    }
}
