use crate::core::{
    message::GuiToPlayerMsg,
    state::{LoopState, ToniqueProjectState},
};

const MIN_LOOP_SIZE: f32 = 4.;

impl ToniqueProjectState {
    /// Get the current loop state
    pub fn loop_state(&self) -> &LoopState {
        &self.loop_state
    }
    /// Set the start loop position at specified location.
    /// If the new loop size is smaller than MIN_LOOP_SIZE, adjust the start position accordingly.
    pub fn set_loop_start(&mut self, mut pos: f32) {
        let size = self.loop_state.end - pos;
        if size < MIN_LOOP_SIZE {
            pos = self.loop_state.end - MIN_LOOP_SIZE;
        }
        let mut new_state = self.loop_state.clone();
        new_state.start = pos;
        self.update_loop(new_state);
    }
    /// Set the end loop position at specified location.
    /// If the new loop size is smaller than MIN_LOOP_SIZE, adjust the end position accordingly
    pub fn set_loop_end(&mut self, mut pos: f32) {
        let size = pos - self.loop_state.start;
        if size < MIN_LOOP_SIZE {
            pos = self.loop_state.start + MIN_LOOP_SIZE;
        }
        let mut new_state = self.loop_state.clone();
        new_state.end = pos;
        self.update_loop(new_state);
    }
    /// Toggle the loop enabled state
    pub fn toggle_loop(&mut self) {
        let mut new_state = self.loop_state.clone();
        new_state.enabled = !new_state.enabled;
        self.update_loop(new_state);
    }

    fn update_loop(&mut self, new_state: LoopState) {
        if let Ok(_) = self.tx.push(GuiToPlayerMsg::UpdateLoop(new_state.clone())) {
            self.loop_state = new_state
        }
    }
}
