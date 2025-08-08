use egui::{Color32, Key, Painter, Pos2, Rect, Sense, Stroke, Vec2};
use rtrb::Producer;

use crate::{
    components::{
        bottom_panel::UIBottomPanel,
        left_panel::DragPayload,
        track::{HANDLE_HEIGHT, TrackSoloState, UITrack},
    },
    message::GuiToPlayerMsg,
    metrics::{AudioMetrics, GlobalMetrics},
};

pub struct TrackManager {
    pub tracks: Vec<UITrack>,
    pub solo_tracks: Vec<String>,
    pub selected_track: Option<String>,
    pub track_width: f32,
}

impl TrackManager {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            solo_tracks: Vec::new(),
            selected_track: None,
            track_width: 140.,
        }
    }

    pub fn create_track(&mut self, name: &str, tx: &mut Producer<GuiToPlayerMsg>) {
        let id = uuid::Uuid::new_v4().to_string();
        self.tracks.push(UITrack::new(&id, name));
        let _ = tx.push(GuiToPlayerMsg::AddTrack(id));
    }

    pub fn delete_track(&mut self, id: String, tx: &mut Producer<GuiToPlayerMsg>) {
        for (i, track) in self.tracks.iter().enumerate() {
            if track.id == id {
                self.tracks.remove(i);
                let _ = tx.push(GuiToPlayerMsg::RemoveTrack(id));
                break;
            }
        }
    }

    pub fn paint_tracks(&self, painter: &Painter, viewport: Rect) {
        let mut y = viewport.top();
        let solo = !self.solo_tracks.is_empty();

        for track in self.tracks.iter() {
            if !self.solo_tracks.contains(&track.id) && (track.muted || solo) {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(viewport.left(), y),
                        Pos2::new(viewport.right(), y + track.height),
                    ),
                    0.,
                    Color32::from_black_alpha(90),
                );
            }
            y += track.height;
            painter.line(
                vec![
                    Pos2::new(viewport.left(), y + HANDLE_HEIGHT / 2.),
                    Pos2::new(viewport.right(), y + HANDLE_HEIGHT / 2.),
                ],
                Stroke::new(HANDLE_HEIGHT, Color32::from_gray(40)),
            );
            y += HANDLE_HEIGHT
        }
    }

    pub fn find_track_at(&self, rect: Rect, y_pos: f32) -> (Option<usize>, f32) {
        // y position of the top of the track
        let mut track_y = rect.top();
        let mut index = None;
        for (i, track) in self.tracks.iter().enumerate() {
            if y_pos <= track_y + track.height {
                index = Some(i);
                break;
            }
            track_y += track.height + HANDLE_HEIGHT;
        }

        (index, track_y)
    }

    // Track panel at the right
    pub fn track_panel(
        &mut self,
        ui: &mut egui::Ui,
        metrics: &mut GlobalMetrics,
        bottom_panel: &mut UIBottomPanel,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        let mut deleted_tracks = Vec::new();

        let res = ui.vertical(|ui| {
            ui.set_width(self.track_width);
            for track in self.tracks.iter_mut() {
                let metrics = metrics.tracks.get(&track.id);
                let metrics = if let Some(metrics) = metrics {
                    metrics.value().clone()
                } else {
                    AudioMetrics::new()
                };
                let solo = if self.solo_tracks.is_empty() {
                    TrackSoloState::NotSoloing
                } else if self.solo_tracks.contains(&track.id) {
                    TrackSoloState::Solo
                } else {
                    TrackSoloState::Soloing
                };
                let selected = self.selected_track.clone().is_some_and(|id| id == track.id);
                // Render track
                let (
                    mute_changed,
                    volume_changed,
                    solo_clicked,
                    clicked,
                    double_clicked,
                    track_res,
                ) = track.ui(ui, metrics, solo, selected);
                // Track events
                if mute_changed {
                    let _ = tx.push(GuiToPlayerMsg::MuteTrack(track.id.clone(), track.muted));
                };
                if volume_changed {
                    let _ = tx.push(GuiToPlayerMsg::ChangeTrackVolume(
                        track.id.clone(),
                        track.volume,
                    ));
                }
                if solo_clicked {
                    if ui.input(|i| i.modifiers.shift) {
                        if self.solo_tracks.contains(&track.id) {
                            self.solo_tracks.retain(|t| *t != track.id);
                        } else {
                            self.solo_tracks.push(track.id.clone());
                        }
                    } else {
                        self.solo_tracks = if self.solo_tracks.contains(&track.id) {
                            vec![]
                        } else {
                            vec![track.id.clone()]
                        }
                    }

                    let _ = tx.push(GuiToPlayerMsg::SoloTracks(self.solo_tracks.clone()));
                }
                if clicked {
                    if selected {
                        self.selected_track = None;
                        track_res.surrender_focus();
                    } else {
                        self.selected_track = Some(track.id.clone());
                        track_res.request_focus();
                    }
                }
                if track_res.has_focus() && ui.input(|i| i.key_pressed(Key::Delete)) {
                    deleted_tracks.push(track.id.clone());
                }
                // Open bottom panel
                if double_clicked {
                    if let Some(selected_id) = self.selected_track.clone()
                        && *selected_id == track.id
                    {
                        bottom_panel.open = !bottom_panel.open;
                    } else {
                        bottom_panel.open = true;
                        self.selected_track = Some(track.id.clone())
                    }
                }
                // // Insert effects
                if let Some(payload) = track_res.dnd_release_payload::<DragPayload>()
                    && let DragPayload::Effect(id) = *payload
                {
                    track.add_effect(id, 0, tx);
                }
            }
        });

        // context menu
        ui.interact(
            Rect::from_min_size(
                res.response.rect.left_bottom(),
                Vec2::new(self.track_width, ui.available_height()),
            ),
            "tracks".into(),
            Sense::click(),
        )
        .context_menu(|ui| {
            if ui.button("Add track").clicked() {
                let name = format!("Track {}", self.tracks.len() + 1).to_string();
                self.create_track(name.as_str(), tx);
                ui.close();
            }
        });

        for track_id in deleted_tracks {
            self.delete_track(track_id, tx);
        }
    }
}
