use egui::{FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use egui_phosphor::fill::PLUS;
use tonique_engine::edit::TrackId;

use crate::{
    config::keymap::Action,
    core::{
        state::{RowTarget, ToniqueProjectState},
        track::{TrackKind, TrackReferenceCore},
    },
    ui::theme::ThemeExt,
    ui::{
        font::PHOSPHOR_REGULAR,
        panels::{central_panel::SCROLLBAR_WIDTH, left_panel::DragPayload},
        track::{COLOR_BAR_WIDTH, HEADER_INSET, ROW_GAP, TrackHeader, color_bar_x},
        widget::{context_menu::ContextMenuButton, square_button::SquareButton},
        workspace::Workspace,
    },
};

pub const SPLITTER_WIDTH: f32 = 2.0;
const DEFAULT_TRACK_WIDTH: f32 = 150.;

/// Share of a group header's height, from the top, where a drop goes above
/// the group rather than into it.
const ABOVE_GROUP: f32 = 0.3;

pub struct TrackHeaders {
    pub width: f32,
    /// The row being dragged to a new place.
    dragging: Option<TrackId>,
}

impl TrackHeaders {
    pub fn new() -> Self {
        Self {
            width: DEFAULT_TRACK_WIDTH,
            dragging: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        workspace: &mut Workspace,
        viewport: Rect,
    ) {
        let left = viewport.max.x - self.width;

        let dragger_rect = Rect::from_min_size(
            pos2(left, viewport.top()),
            vec2(SPLITTER_WIDTH, viewport.height()),
        );
        let response = ui.allocate_rect(dragger_rect, Sense::drag());

        let painter = ui.painter_at(dragger_rect);

        let theme = ui.app_theme();
        let color = if response.hovered() || response.dragged() {
            theme.accent
        } else {
            theme.separator
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
                    pos2(left + SPLITTER_WIDTH, viewport.top() - state.grid.offset.y),
                    pos2(viewport.right() - SCROLLBAR_WIDTH, viewport.bottom()),
                ))
                .id_salt("track-area"),
            |ui| {
                self.track_panel(ui, state, workspace, viewport);
            },
        );
        let master_track = state.master_track();
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(
                    left + SPLITTER_WIDTH,
                    viewport.bottom() - master_track.height,
                ),
                pos2(viewport.right() - SCROLLBAR_WIDTH, viewport.bottom()),
            )),
            |ui| {
                ui.horizontal(|ui| {
                    TrackHeader::new().ui(ui, &master_track, state);
                })
            },
        );
    }

    pub fn track_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ToniqueProjectState,
        workspace: &mut Workspace,
        viewport: Rect,
    ) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());

            let rows = state.rows();
            let mut y = viewport.top();
            let (left, width) = (ui.max_rect().left(), ui.available_width());
            // Where each row is on screen, for dropping.
            let mut placed = Vec::with_capacity(rows.len());
            for track in rows {
                let track_bottom = y + track.height;
                let view_top = viewport.top() + state.grid.offset.y;
                let view_bottom = view_top + viewport.height();
                placed.push((
                    track.clone(),
                    Rect::from_min_size(
                        pos2(left, y - state.grid.offset.y),
                        vec2(width, track.height),
                    ),
                ));

                // Skip if track is entirely outside the visible vertical range
                if track_bottom < view_top || y > view_bottom {
                    y += track.height + ROW_GAP;
                    ui.add_space(track.height + ROW_GAP);
                    continue;
                }

                let response = TrackHeader::new().ui(ui, &track, state);
                if response.drag_started() {
                    self.dragging = Some(track.id);
                }
                if response.dragged() {
                    ui.painter()
                        .rect_filled(response.rect, 1.0, ui.app_theme().hover_overlay);
                }
                // Open bottom panel
                if response.double_clicked() {
                    if track.selected {
                        workspace.bottom_panel_open = !workspace.bottom_panel_open;
                    } else {
                        workspace.bottom_panel_open = true;
                    }
                }
                // Insert effects
                if let Some(payload) = response.dnd_release_payload::<DragPayload>()
                    && let DragPayload::Effect(id) = *payload
                {
                    state.add_effect(&track.id, id, 0);
                }

                y += track.height + ROW_GAP;
            }
            paint_group_scopes(ui, &placed);
            self.drop_rows(ui, state, &placed);

            if ui
                .add(
                    SquareButton::ghost(format!("{}", PLUS))
                        .font(FontId::new(
                            10.,
                            egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                        ))
                        .size(vec2(ui.available_width(), 20.))
                        .tooltip(state.settings().keymap.with_shortcut(
                            ui.ctx(),
                            "Add audio track",
                            Action::AddTrack,
                        )),
                )
                .clicked()
            {
                state.add_track();
            }
        });

        let (_, res) = ui.allocate_at_least(
            vec2(ui.available_width(), ui.available_height().max(200.)),
            Sense::click(),
        );

        if res.clicked() {
            state.clear_track_selection();
        }

        res.context_menu(|ui| {
            self.context_menu_ui(ui, state);
        });
    }

    /// While a row is dragged, show where it would land; move it there on
    /// release.
    fn drop_rows(
        &mut self,
        ui: &Ui,
        state: &mut ToniqueProjectState,
        placed: &[(TrackReferenceCore, Rect)],
    ) {
        let Some(dragged) = self.dragging else {
            return;
        };
        let target = ui
            .input(|i| i.pointer.interact_pos())
            .and_then(|pointer| drop_target(placed, pointer.y))
            .filter(|(target, _)| match target {
                RowTarget::Before(row) | RowTarget::Into(row) => {
                    *row != dragged && !state.ancestors(*row).contains(&dragged)
                }
                RowTarget::End => true,
            });
        if let Some((target, marker)) = target {
            let accent = ui.app_theme().accent;
            match target {
                RowTarget::Into(_) => {
                    ui.painter().rect_stroke(
                        marker,
                        2.,
                        Stroke::new(2., accent),
                        StrokeKind::Inside,
                    );
                }
                _ => {
                    ui.painter()
                        .hline(marker.x_range(), marker.top(), Stroke::new(2., accent));
                }
            }
        }
        if ui.input(|i| i.pointer.any_released()) {
            self.dragging = None;
            if let Some((target, _)) = target {
                state.move_row(dragged, target);
            }
        }
    }

    fn context_menu_ui(&self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        if ui
            .add(ContextMenuButton::new(PLUS, "Add audio track"))
            .clicked()
        {
            state.add_track();
            ui.close();
        }
    }
}

/// Where a row dropped at `y` lands, and where to mark it: the row's rect
/// for dropping into a group, else a line at the rect's top.
fn drop_target(placed: &[(TrackReferenceCore, Rect)], y: f32) -> Option<(RowTarget, Rect)> {
    for (i, (row, rect)) in placed.iter().enumerate() {
        if y > rect.bottom() + ROW_GAP {
            continue;
        }
        let share = (y - rect.top()) / rect.height().max(1.);
        let target = if row.kind == TrackKind::Group && share > ABOVE_GROUP {
            (RowTarget::Into(row.id), *rect)
        } else if row.kind == TrackKind::Group || share < 0.5 {
            (RowTarget::Before(row.id), *rect)
        } else {
            match placed.get(i + 1) {
                Some((next, next_rect)) => (RowTarget::Before(next.id), *next_rect),
                None => (RowTarget::End, rect.translate(vec2(0., rect.height()))),
            }
        };
        return Some(target);
    }
    let (_, last) = placed.last()?;
    Some((RowTarget::End, last.translate(vec2(0., last.height()))))
}

/// Extend each expanded group's coloured bar down to its last row, so its
/// scope shows.
fn paint_group_scopes(ui: &Ui, placed: &[(TrackReferenceCore, Rect)]) {
    let theme = ui.app_theme();
    for (i, (group, rect)) in placed.iter().enumerate() {
        if group.kind != TrackKind::Group || group.collapsed {
            continue;
        }
        let Some((_, last)) = placed[i + 1..]
            .iter()
            .take_while(|(row, _)| row.depth > group.depth)
            .last()
        else {
            continue;
        };
        let x = color_bar_x(rect.left(), group.depth);
        let bar = Rect::from_min_max(
            pos2(x, rect.top() + HEADER_INSET),
            pos2(x + COLOR_BAR_WIDTH, last.bottom() - HEADER_INSET),
        );
        let color = if group.disabled() {
            theme.text_disabled
        } else {
            group.color
        };
        ui.painter().rect_filled(bar, 0., color);
    }
}
