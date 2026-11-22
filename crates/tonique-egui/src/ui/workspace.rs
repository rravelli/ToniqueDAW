//! How the window is laid out: which panels are open and what the main
//! area shows. Not part of the project.

use tonique_engine::edit::ClipId;

use crate::core::state::ProjectState;

/// What the main area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainView {
    Timeline,
    /// The engine's processing graph, live.
    Graph,
}

/// What the bottom panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BottomView {
    /// The selected track's effects.
    #[default]
    Effects,
    /// Details of this clip, while it exists.
    Clip(ClipId),
}

pub struct Workspace {
    /// The browser, on the left.
    pub left_panel_open: bool,
    /// The selected track's effects, at the bottom.
    pub bottom_panel_open: bool,
    /// The track headers, right of the timeline; a strip of the tracks'
    /// colours when collapsed.
    pub track_list_open: bool,
    pub bottom_view: BottomView,
    pub main_view: MainView,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            left_panel_open: true,
            bottom_panel_open: false,
            track_list_open: true,
            bottom_view: BottomView::Effects,
            main_view: MainView::Timeline,
        }
    }
}

impl Workspace {
    /// Between the timeline and the graph.
    pub fn toggle_graph(&mut self) {
        self.main_view = match self.main_view {
            MainView::Graph => MainView::Timeline,
            MainView::Timeline => MainView::Graph,
        };
    }

    /// Open the bottom panel on the details of clip `id`, and select its
    /// track.
    pub fn show_clip_details(&mut self, state: &mut ProjectState, id: ClipId) {
        if let Some((track, _)) = state.find_clip(id) {
            state.select_track(&track);
            self.bottom_view = BottomView::Clip(id);
            self.bottom_panel_open = true;
        }
    }

    /// Show `view` in the bottom panel.
    pub fn show_view(&mut self, state: &mut ProjectState, view: BottomView) {
        match view {
            BottomView::Clip(id) => self.show_clip_details(state, id),
            BottomView::Effects => self.bottom_view = BottomView::Effects,
        }
    }

    /// While the bottom panel shows a clip, show the one selected instead,
    /// when exactly one is; and the track's effects once the clip is gone.
    pub fn follow_clip_selection(&mut self, state: &ProjectState) {
        let BottomView::Clip(shown) = self.bottom_view else {
            return;
        };
        if let [selected] = state.selected_clips()
            && *selected != shown
            && state.find_clip(*selected).is_some()
        {
            self.bottom_view = BottomView::Clip(*selected);
        } else if state.find_clip(shown).is_none() {
            self.bottom_view = BottomView::Effects;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analysis::AudioInfo, core::clip::AudioClip};
    use tonique_engine::{
        engine::{Engine, EngineConfig},
        time::BeatPos,
    };

    fn state_with_clips() -> (ProjectState, ClipId, ClipId) {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let track = state.add_track();
        let (a, b) = (state.new_clip_id(), state.new_clip_id());
        state.add_clips(
            &track,
            vec![
                AudioClip::new(a, AudioInfo::silent(1.), BeatPos(0.), 120.),
                AudioClip::new(b, AudioInfo::silent(1.), BeatPos(4.), 120.),
            ],
        );
        (state, a, b)
    }

    #[test]
    fn the_clip_view_follows_a_single_selected_clip() {
        let (mut state, a, b) = state_with_clips();
        let mut workspace = Workspace::default();
        workspace.show_clip_details(&mut state, a);
        assert!(workspace.bottom_panel_open);
        assert_eq!(workspace.bottom_view, BottomView::Clip(a));

        state.select_clips(vec![b]);
        workspace.follow_clip_selection(&state);
        assert_eq!(workspace.bottom_view, BottomView::Clip(b));

        // Several, or none: it stays.
        state.select_clips(vec![a, b]);
        workspace.follow_clip_selection(&state);
        assert_eq!(workspace.bottom_view, BottomView::Clip(b));
        state.clear_clip_selection();
        workspace.follow_clip_selection(&state);
        assert_eq!(workspace.bottom_view, BottomView::Clip(b));

        // The effects don't follow it.
        workspace.show_view(&mut state, BottomView::Effects);
        state.select_clips(vec![a]);
        workspace.follow_clip_selection(&state);
        assert_eq!(workspace.bottom_view, BottomView::Effects);
    }

    #[test]
    fn a_deleted_clip_gives_way_to_the_effects_for_good() {
        let (mut state, a, _) = state_with_clips();
        let mut workspace = Workspace::default();
        workspace.show_clip_details(&mut state, a);
        state.delete_clips(&[a]);
        workspace.follow_clip_selection(&state);
        assert_eq!(workspace.bottom_view, BottomView::Effects);
        // Undo brings the clip back, not its view.
        state.undo();
        workspace.follow_clip_selection(&state);
        assert_eq!(workspace.bottom_view, BottomView::Effects);
    }
}
