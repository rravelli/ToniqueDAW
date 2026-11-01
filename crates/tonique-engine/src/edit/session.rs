use super::builder::{automation_curve, automation_identity, build_graph};
use super::commands::EditCommand;
use std::sync::Arc;

use super::{Edit, EditError, Effects, SourceId, UndoManager};
use crate::engine::{Command, Engine, EngineError};
use crate::graph::{CompileStats, NodeMessage};
use crate::sample::SampleBuffer;
use crate::stretch::Stretchers;
use crate::time::BeatPos;

impl From<EngineError> for EditError {
    fn from(e: EngineError) -> Self {
        EditError::Engine(e.to_string())
    }
}

/// An edit bound to a running engine: every change goes through the undo
/// manager, and its effects are pushed to the engine (graph rebuild, curve
/// swap, or nothing at all for atomic parameter changes).
pub struct EditSession {
    edit: Edit,
    undo: UndoManager,
    engine: Engine,
    last_stats: Option<CompileStats>,
    stretchers: Stretchers,
    /// Effects held back by [`Self::hold_effects`], and how many holds are
    /// open.
    held: Option<(Effects, usize)>,
}

impl EditSession {
    pub fn new(edit: Edit, engine: Engine) -> Result<Self, EditError> {
        edit.refresh_mute_gains();
        let mut s = Self {
            edit,
            undo: UndoManager::default(),
            engine,
            last_stats: None,
            stretchers: Stretchers::default(),
            held: None,
        };
        s.rebuild()?;
        Ok(s)
    }

    /// Replace the edit (a new or loaded project) on the same engine, with an
    /// empty undo history. The transport is left stopped.
    pub fn reset(&mut self, edit: Edit) -> Result<(), EditError> {
        self.engine.stop()?;
        edit.refresh_mute_gains();
        self.edit = edit;
        self.undo.clear();
        self.rebuild()?;
        Ok(())
    }

    /// Forget the undo history, keeping the edit.
    pub fn clear_history(&mut self) {
        self.undo.clear();
    }

    pub fn edit(&self) -> &Edit {
        &self.edit
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Move the edit to a new engine (e.g. after changing the audio device
    /// or engine settings) and return the old one. The undo history is kept;
    /// the transport starts stopped at 0 on the new engine.
    pub fn replace_engine(&mut self, engine: Engine) -> Result<Engine, EditError> {
        let old = std::mem::replace(&mut self.engine, engine);
        // Nothing to hand stretchers over from.
        self.stretchers = Stretchers::default();
        self.rebuild()?;
        Ok(old)
    }

    /// Construct new model objects (tracks, clips, plugins...), which draws
    /// fresh IDs from the edit. Add them with commands; changes made to the
    /// edit here directly are not undoable and don't reach the engine.
    pub fn create<R>(&mut self, f: impl FnOnce(&mut Edit) -> R) -> R {
        f(&mut self.edit)
    }

    /// Provide audio for clips that reference `id` (see [`Edit::new_source`])
    /// and rebuild. Not part of the undo history.
    pub fn set_source(&mut self, id: SourceId, data: Arc<SampleBuffer>) -> Result<(), EditError> {
        self.edit.set_source(id, data);
        self.rebuild().map(|_| ())
    }

    pub fn undo_manager(&self) -> &UndoManager {
        &self.undo
    }

    pub fn last_compile_stats(&self) -> Option<CompileStats> {
        self.last_stats
    }

    pub fn perform(&mut self, cmd: impl EditCommand + 'static) -> Result<(), EditError> {
        let effects = self.undo.perform(&mut self.edit, Box::new(cmd))?;
        self.apply_effects(effects)
    }

    pub fn begin_transaction(&mut self, label: &'static str) {
        self.undo.begin_transaction(label);
    }

    pub fn commit_transaction(&mut self) {
        self.undo.commit_transaction();
    }

    pub fn cancel_transaction(&mut self) -> Result<(), EditError> {
        let e = self.undo.cancel_transaction(&mut self.edit)?;
        self.apply_effects(e)
    }

    /// Returns false if there was nothing to undo.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        match self.undo.undo(&mut self.edit)? {
            Some(e) => self.apply_effects(e).map(|_| true),
            None => Ok(false),
        }
    }

    pub fn redo(&mut self) -> Result<bool, EditError> {
        match self.undo.redo(&mut self.edit)? {
            Some(e) => self.apply_effects(e).map(|_| true),
            None => Ok(false),
        }
    }

    /// Hold back the effects of the changes made until
    /// [`Self::release_effects`], to push them to the engine at once (one
    /// rebuild for many clips added...). Holds nest.
    pub fn hold_effects(&mut self) {
        match &mut self.held {
            Some((_, holds)) => *holds += 1,
            None => self.held = Some((Effects::none(), 1)),
        }
    }

    /// Close a hold, pushing the effects held back once the last is.
    pub fn release_effects(&mut self) -> Result<(), EditError> {
        match self.held.take() {
            Some((effects, 1)) => self.apply_effects(effects),
            Some((effects, holds)) => {
                self.held = Some((effects, holds - 1));
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn apply_effects(&mut self, effects: Effects) -> Result<(), EditError> {
        if let Some((held, _)) = &mut self.held {
            held.merge(effects);
            return Ok(());
        }
        if effects.rebuild {
            return self.rebuild().map(|_| ());
        }
        let sr = self.engine.config().sample_rate;
        for id in effects.curves {
            let curve = automation_curve(&self.edit, self.edit.param(id)?, sr);
            self.engine.send(Command::SendToNode {
                target: automation_identity(id),
                msg: NodeMessage::Curve(curve),
            })?;
        }
        Ok(())
    }

    /// Rebuild the graph from the edit and hand it to the engine.
    pub fn rebuild(&mut self) -> Result<CompileStats, EditError> {
        // A graph still pending now is replaced, never playing: it can't
        // hand its stretchers over.
        let handover = !self.engine.has_pending_graph();
        let sr = self.engine.config().sample_rate;
        self.stretchers.begin(sr, handover);
        let desc = build_graph(&self.edit, sr, &mut self.stretchers);
        let stats = self.engine.load_graph(desc)?;
        self.stretchers.published();
        self.last_stats = Some(stats);
        Ok(stats)
    }

    fn to_samples(&self, b: BeatPos) -> i64 {
        self.edit
            .tempo
            .beats_to_samples(b, self.engine.config().sample_rate)
    }

    pub fn play(&mut self) -> Result<(), EditError> {
        Ok(self.engine.play()?)
    }

    pub fn stop(&mut self) -> Result<(), EditError> {
        Ok(self.engine.stop()?)
    }

    pub fn seek(&mut self, beat: BeatPos) -> Result<(), EditError> {
        let pos = self.to_samples(beat);
        Ok(self.engine.seek(pos)?)
    }

    pub fn set_loop(&mut self, range: Option<(BeatPos, BeatPos)>) -> Result<(), EditError> {
        let r = range.map(|(a, b)| (self.to_samples(a), self.to_samples(b)));
        Ok(self.engine.set_loop(r)?)
    }

    pub fn position(&self) -> BeatPos {
        self.edit
            .tempo
            .samples_to_beats(self.engine.position(), self.engine.config().sample_rate)
    }
}
