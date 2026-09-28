use egui::{FontId, Rangef, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use egui_phosphor::fill::PLUS;
use tonique_engine::edit::TrackId;

use crate::{
    config::keymap::Action,
    core::{
        state::{ProjectState, RowTarget},
        track::TrackKind,
    },
    ui::theme::ThemeExt,
    ui::{
        font::PHOSPHOR_REGULAR,
        panels::{central_panel::SCROLLBAR_WIDTH, left_panel::DragPayload},
        track::{COLOR_BAR_WIDTH, HEADER_INSET, ROW_GAP, TrackHeader, color_bar_x},
        view::row_layout::{Row, RowLayout},
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
        state: &mut ProjectState,
        workspace: &mut Workspace,
        layout: &RowLayout,
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
                self.track_panel(ui, state, workspace, layout, viewport);
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
        state: &mut ProjectState,
        workspace: &mut Workspace,
        layout: &RowLayout,
        viewport: Rect,
    ) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());

            // The headers' column, for dropping rows.
            let x = Rangef::new(
                ui.max_rect().left(),
                ui.max_rect().left() + ui.available_width(),
            );
            for row in layout.rows() {
                let track = &row.track;
                // Skip if track is entirely outside the visible vertical range
                if row.y.max < viewport.top() || row.y.min > viewport.bottom() {
                    ui.add_space(track.height + ROW_GAP);
                    continue;
                }

                let response = TrackHeader::new().ui(ui, track, state);
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
            }
            paint_group_scopes(ui, layout.rows(), x);
            self.drop_rows(ui, state, layout.rows(), x);

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
    fn drop_rows(&mut self, ui: &Ui, state: &mut ProjectState, rows: &[Row], x: Rangef) {
        let Some(dragged) = self.dragging else {
            return;
        };
        let target = ui
            .input(|i| i.pointer.interact_pos())
            .and_then(|pointer| drop_target(rows, x, pointer.y))
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

    fn context_menu_ui(&self, ui: &mut Ui, state: &mut ProjectState) {
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
fn drop_target(rows: &[Row], x: Rangef, y: f32) -> Option<(RowTarget, Rect)> {
    let rect = |row: &Row| Rect::from_x_y_ranges(x, row.y);
    let below = |rect: Rect| rect.translate(vec2(0., rect.height()));
    for (i, row) in rows.iter().enumerate() {
        if y > row.y.max + ROW_GAP {
            continue;
        }
        let track = &row.track;
        let share = (y - row.y.min) / row.y.span().max(1.);
        let target = if track.kind == TrackKind::Group && share > ABOVE_GROUP {
            (RowTarget::Into(track.id), rect(row))
        } else if track.kind == TrackKind::Group || share < 0.5 {
            (RowTarget::Before(track.id), rect(row))
        } else {
            match rows.get(i + 1) {
                Some(next) => (RowTarget::Before(next.track.id), rect(next)),
                None => (RowTarget::End, below(rect(row))),
            }
        };
        return Some(target);
    }
    Some((RowTarget::End, below(rect(rows.last()?))))
}

/// Extend each expanded group's coloured bar down to its last row, so its
/// scope shows.
fn paint_group_scopes(ui: &Ui, rows: &[Row], x: Rangef) {
    let theme = ui.app_theme();
    for row in rows {
        let group = &row.track;
        // Only expanded groups with something inside.
        if group.kind != TrackKind::Group || group.collapsed || row.scope == row.y {
            continue;
        }
        let x = color_bar_x(x.min, group.depth);
        let bar = Rect::from_min_max(
            pos2(x, row.y.min + HEADER_INSET),
            pos2(x + COLOR_BAR_WIDTH, row.scope.max - HEADER_INSET),
        );
        let color = if group.disabled() {
            theme.text_disabled
        } else {
            group.color
        };
        ui.painter().rect_filled(bar, 0., color);
    }
}
