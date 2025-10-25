use egui::{Rect, Sense, Vec2};
use egui_phosphor::fill::PLUS;

use crate::{
    core::{
        state::ToniqueProjectState,
        track::{TrackCore, TrackReferenceCore},
    },
    ui::{
        panels::bottom_panel::UIBottomPanel,
        panels::left_panel::DragPayload,
        track::{DEFAULT_TRACK_HEIGHT, HANDLE_HEIGHT, UITrack},
        widget::context_menu::ContextMenuButton,
    },
};

pub struct TrackManager {
    pub track_width: f32,
}

impl TrackManager {
    pub fn new() -> Self {
        Self { track_width: 140. }
    }

    pub fn get_track_y(
        &self,
        track_index: usize,
        viewport: Rect,
        state: &ToniqueProjectState,
    ) -> f32 {
        let mut y = viewport.top();
        let mut curr_index = 0;
        for track in state.tracks() {
            if curr_index == track_index {
                return y;
            }
            curr_index += 1;
            y += track.height + HANDLE_HEIGHT;
        }
        return (track_index - curr_index) as f32 * (DEFAULT_TRACK_HEIGHT + HANDLE_HEIGHT) + y;
    }

    pub fn find_track_at(
        &self,
        viewport: Rect,
        state: &ToniqueProjectState,
        y: f32,
    ) -> (Option<TrackReferenceCore>, f32) {
        // y position of the top of the track
        let mut track_y = viewport.top();
        let mut track = None;
        for t in state.tracks() {
            if y <= track_y + t.height {
                track = Some(t);
                break;
            }
            track_y += t.height + HANDLE_HEIGHT;
        }

        (track, track_y)
    }

    // Track panel at the right
    pub fn track_panel(
        &mut self,
        ui: &mut egui::Ui,
        bottom_panel: &mut UIBottomPanel,
        state: &mut ToniqueProjectState,
    ) {
        let res = ui.vertical(|ui| {
            ui.set_width(self.track_width);

            let tracks: Vec<_> = state.tracks().collect();

            for track in tracks {
                let response = UITrack::new().ui(ui, &track, state);
                // Open bottom panel
                if response.double_clicked() {
                    if track.selected {
                        bottom_panel.open = !bottom_panel.open;
                    } else {
                        bottom_panel.open = true;
                    }
                }
                // Insert effects
                if let Some(payload) = response.dnd_release_payload::<DragPayload>()
                    && let DragPayload::Effect(id) = *payload
                {
                    state.add_effect(&track.id, id, 0);
                }
            }
        });

        // context menu
        let res = ui.interact(
            Rect::from_min_size(
                res.response.rect.left_bottom(),
                Vec2::new(self.track_width, ui.available_height()),
            ),
            "tracks".into(),
            Sense::click(),
        );

        if res.clicked() {
            state.deselect();
        }

        res.context_menu(|ui| {
            if ui.add(ContextMenuButton::new(PLUS, "Add track")).clicked() {
                state.add_track(TrackCore::new());
                ui.close();
            }
        });
    }
}
