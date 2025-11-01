use egui::{Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use egui_phosphor::fill::PLUS;

use crate::{
    core::{state::ToniqueProjectState, track::TrackCore},
    ui::{
        font::PHOSPHOR_REGULAR,
        panels::{central_panel::SCROLLBAR_WIDTH, left_panel::DragPayload},
        track::{HANDLE_HEIGHT, UITrack},
        utils::find_track_at,
        widget::{context_menu::ContextMenuButton, square_button::SquareButton},
    },
};

pub const DRAGGER_WIDTH: f32 = 2.0;
const DEFAULT_TRACK_WIDTH: f32 = 150.;

pub struct UITracks {
    pub width: f32,
}

impl UITracks {
    pub fn new() -> Self {
        Self {
            width: DEFAULT_TRACK_WIDTH,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState, viewport: Rect) {
        let left = viewport.max.x - self.width;

        let dragger_rect = Rect::from_min_size(
            pos2(left, viewport.top()),
            vec2(DRAGGER_WIDTH, viewport.height()),
        );
        let response = ui.allocate_rect(dragger_rect, Sense::drag());

        let painter = ui.painter_at(dragger_rect);

        let color = if response.hovered() || response.dragged() {
            Color32::WHITE
        } else {
            Color32::from_gray(70)
        };
        painter.rect_filled(dragger_rect, 0., color);
        if response.dragged() {
            self.width -= response.drag_delta().x;
            self.width = self.width.clamp(120., ui.available_width());
        }
        response.on_hover_and_drag_cursor(egui::CursorIcon::ResizeHorizontal);

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(left + DRAGGER_WIDTH, viewport.top() - state.grid.offset.y),
                    pos2(viewport.right() - SCROLLBAR_WIDTH, viewport.bottom()),
                ))
                .id_salt("track-area"),
            |ui| {
                self.track_panel(ui, state, viewport);
            },
        );
        let master_track = state.master_track();
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(
                    left + DRAGGER_WIDTH,
                    viewport.bottom() - master_track.height,
                ),
                pos2(viewport.right() - SCROLLBAR_WIDTH, viewport.bottom()),
            )),
            |ui| {
                ui.horizontal(|ui| {
                    UITrack::new().ui(ui, &master_track, state);
                })
            },
        );
    }

    pub fn track_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
    ) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());

            let tracks: Vec<_> = state.tracks().collect();
            let mut y = viewport.top();
            let mut dragged_track = None;
            for track in tracks {
                let track_bottom = y + track.height;
                let view_top = viewport.top() + state.grid.offset.y;
                let view_bottom = view_top + viewport.height();

                // Skip if track is entirely outside the visible vertical range
                if track_bottom < view_top || y > view_bottom {
                    y += track.height + HANDLE_HEIGHT;
                    ui.add_space(track.height + HANDLE_HEIGHT);
                    continue;
                }

                let response = UITrack::new().ui(ui, &track, state);
                if response.dragged() {
                    dragged_track = Some(track.clone());
                    ui.painter()
                        .rect_filled(response.rect, 1.0, Color32::from_white_alpha(20));
                }
                // Open bottom panel
                if response.double_clicked() {
                    if track.selected {
                        state.bottom_panel_open = !state.bottom_panel_open;
                    } else {
                        state.bottom_panel_open = true;
                    }
                }
                // Insert effects
                if let Some(payload) = response.dnd_release_payload::<DragPayload>()
                    && let DragPayload::Effect(id) = *payload
                {
                    state.add_effect(&track.id, id, 0);
                }

                y += track.height + HANDLE_HEIGHT;
            }
            if let Some(track) = dragged_track
                && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
            {
                let (t, _) = find_track_at(state, viewport, pos.y);
                state.move_track(&track.id, t.map_or(state.track_len() - 1, |t| t.index));
            }

            if ui
                .add(
                    SquareButton::ghost(format!("{}", PLUS))
                        .font(FontId::new(
                            10.,
                            egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                        ))
                        .size(vec2(ui.available_width(), 20.))
                        .tooltip("Add audio track"),
                )
                .clicked()
            {
                state.add_track(TrackCore::new());
            }
        });

        let (_, res) = ui.allocate_at_least(
            vec2(ui.available_width(), ui.available_height().max(200.)),
            Sense::click(),
        );

        if res.clicked() {
            state.deselect();
        }

        res.context_menu(|ui| {
            self.context_menu_ui(ui, state);
        });
    }

    fn context_menu_ui(&self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        if ui
            .add(ContextMenuButton::new(PLUS, "Add audio track"))
            .clicked()
        {
            state.add_track(TrackCore::new());
            ui.close();
        }
    }
}
