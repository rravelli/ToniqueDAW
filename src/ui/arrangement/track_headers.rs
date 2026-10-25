use egui::{Color32, FontId, Rangef, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use egui_phosphor::regular::PLUS;
use tonique_engine::edit::TrackId;

use crate::{
    config::keymap::Action,
    core::{
        state::{ProjectState, RowTarget, TRACK_NAME},
        track::{DEFAULT_TRACK_HEIGHT, TrackKind, TrackRow, TrackSoloState},
    },
    ui::theme::{ThemeExt, font_size, with_alpha},
    ui::{
        arrangement::row_layout::{Row, RowLayout},
        arrangement::track_header::{
            COLOR_BAR_WIDTH, HEADER_INSET, ROW_GAP, TrackHeader, color_bar_x, ghost_header,
        },
        dnd::DragPayload,
        widget::{context_menu::ContextMenuButton, flat_button::FlatButton},
        workspace::{BottomView, Workspace},
    },
};

pub const SPLITTER_WIDTH: f32 = 2.0;
const DEFAULT_TRACK_WIDTH: f32 = 190.;
const MIN_WIDTH: f32 = 130.;
/// Between the panel's edges and the headers.
const PANEL_MARGIN: f32 = 3.;
/// How opaque the accent is over a row an effect would be dropped on.
const DROP_FILL_ALPHA: u8 = 40;
const ADD_TRACK_HEIGHT: f32 = 24.;
/// Above the master's header, below the footer's top line.
pub const FOOTER_PADDING: f32 = 3.;

/// Share of a group header's height, from the top, where a drop goes above
/// the group rather than into it.
const ABOVE_GROUP: f32 = 0.3;

pub struct TrackHeaders {
    pub width: f32,
    /// The row being dragged to a new place.
    dragging: Option<TrackId>,
    /// The track to start renaming, next drawn.
    rename: Option<TrackId>,
}

impl TrackHeaders {
    pub fn new() -> Self {
        Self {
            width: DEFAULT_TRACK_WIDTH,
            dragging: None,
            rename: None,
        }
    }

    /// Start typing a new name for `track`, if it's on screen.
    pub fn rename(&mut self, track: TrackId) {
        self.rename = Some(track);
    }

    /// The track list: a panel of its own at the right of `viewport` (the
    /// tracks' part of the central panel, timeline and headers), from the
    /// ruler's top (`top`) to the footer's bottom, resized by dragging its
    /// left edge. `lanes` is the timeline's part, where the row an effect
    /// would be dropped on is lit with its header. `new_tracks`: the
    /// colours of the tracks a drop on the timeline would add, shown as
    /// ghost headers below the last.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        layout: &RowLayout,
        viewport: Rect,
        lanes: Rect,
        top: f32,
        footer: Rect,
        new_tracks: &[Color32],
    ) {
        let left = viewport.max.x - self.width;
        let rename = self.rename.take();
        let theme = ui.app_theme();
        // Behind the headers, showing between them.
        ui.painter().rect_filled(
            Rect::from_x_y_ranges(left..=viewport.right(), viewport.y_range()),
            0.,
            theme.bg_panel,
        );

        let dragger_rect =
            Rect::from_x_y_ranges(left..=left + SPLITTER_WIDTH, top..=footer.bottom());
        let response = ui.interact(
            dragger_rect,
            ui.id().with("track-list-splitter"),
            Sense::drag(),
        );
        let color = if response.hovered() || response.dragged() {
            theme.accent
        } else {
            theme.border
        };
        ui.painter()
            .with_clip_rect(dragger_rect)
            .rect_filled(dragger_rect, 0., color);
        if response.dragged() {
            self.width -= response.drag_delta().x;
            // Not clamp: the window may be too narrow for both bounds.
            self.width = self.width.min(viewport.width() / 2.).max(MIN_WIDTH);
        }
        response.on_hover_and_drag_cursor(egui::CursorIcon::ResizeHorizontal);

        let x = left + SPLITTER_WIDTH + PANEL_MARGIN..=viewport.right() - PANEL_MARGIN;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_x_y_ranges(
                    x.clone(),
                    viewport.top() - state.grid.offset.y..=viewport.bottom(),
                ))
                .id_salt("track-area"),
            |ui| {
                self.track_panel(ui, state, workspace, layout, viewport, lanes, rename);
                paint_new_track_headers(ui, state, layout, new_tracks);
            },
        );
        // The master, in the footer below the tracks, under the headers.
        let master_track = state.master_track();
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_x_y_ranges(
                x,
                footer.top() + FOOTER_PADDING..=footer.bottom(),
            )),
            |ui| {
                ui.set_clip_rect(footer);
                ui.horizontal(|ui| {
                    TrackHeader::new().ui(
                        ui,
                        &master_track,
                        state,
                        rename == Some(master_track.id),
                    );
                })
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn track_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        layout: &RowLayout,
        viewport: Rect,
        lanes: Rect,
        rename: Option<TrackId>,
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

                let response = TrackHeader::new().ui(ui, track, state, rename == Some(track.id));
                if response.drag_started() {
                    self.dragging = Some(track.id);
                }
                if response.dragged() {
                    ui.painter()
                        .rect_filled(response.rect, 1.0, ui.app_theme().hover_overlay);
                }
                // Open bottom panel
                if response.double_clicked() {
                    let showing_effects = workspace.bottom_view == BottomView::Effects;
                    if track.selected && showing_effects {
                        workspace.bottom_panel_open = !workspace.bottom_panel_open;
                    } else {
                        workspace.bottom_panel_open = true;
                    }
                    workspace.bottom_view = BottomView::Effects;
                }
                // Effects dropped on the header or the lane: both lit while
                // one's under the pointer. The lane takes the drop itself.
                let lane = Rect::from_x_y_ranges(lanes.x_range(), row.y);
                if dragging_effect(ui)
                    && ui
                        .input(|i| i.pointer.hover_pos())
                        .is_some_and(|p| lane.contains(p) || response.rect.contains(p))
                {
                    paint_drop_target(ui, response.rect, lane);
                }
                if let Some(payload) = response.dnd_release_payload::<DragPayload>()
                    && let DragPayload::Effect(id) = *payload
                {
                    state.add_effect(&track.id, id, usize::MAX);
                    state.select_track(&track.id);
                    workspace.bottom_panel_open = true;
                    workspace.bottom_view = BottomView::Effects;
                }
            }
            paint_group_scopes(ui, layout.rows(), x);
            self.drop_rows(ui, state, layout.rows(), x);

            if ui
                .add(
                    FlatButton::ghost(format!("{PLUS} Add track"))
                        .font(FontId::proportional(font_size::LABEL))
                        .color(ui.app_theme().text_muted)
                        .size(vec2(ui.available_width(), ADD_TRACK_HEIGHT))
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

/// Whether an effect is being dragged.
fn dragging_effect(ui: &Ui) -> bool {
    egui::DragAndDrop::payload::<DragPayload>(ui.ctx())
        .is_some_and(|p| matches!(*p, DragPayload::Effect(_)))
}

/// A row an effect would be dropped on: its header and its lane, lit and
/// outlined in the accent.
fn paint_drop_target(ui: &Ui, header: Rect, lane: Rect) {
    let accent = ui.app_theme().accent;
    let (fill, stroke) = (
        with_alpha(accent, DROP_FILL_ALPHA),
        Stroke::new(1.5, accent),
    );
    for (rect, clip) in [
        (header, ui.clip_rect()),
        (lane, ui.clip_rect().intersect(lane)),
    ] {
        let painter = ui.painter().with_clip_rect(clip);
        painter.rect_filled(rect, 2., fill);
        painter.rect_stroke(rect, 2., stroke, StrokeKind::Inside);
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
        let color = if group.is_silenced() {
            theme.text_disabled
        } else {
            group.color
        };
        ui.painter().rect_filled(bar, 0., color);
    }
}

/// A ghost header for each track a drop would add below the last, as it
/// will look once added. Opaque, over the "Add track" button.
fn paint_new_track_headers(
    ui: &mut Ui,
    state: &ProjectState,
    layout: &RowLayout,
    colors: &[Color32],
) {
    let x = ui.max_rect().x_range();
    for (i, color) in colors.iter().enumerate() {
        let index = state.track_count() + i;
        let top = layout.track_top(index);
        let rect = Rect::from_x_y_ranges(x, top..=top + DEFAULT_TRACK_HEIGHT);
        let track = TrackRow {
            id: TrackId(u64::MAX),
            clips: Vec::new(),
            muted: false,
            volume: 1.,
            pan: 0.,
            armed: false,
            name: TRACK_NAME.into(),
            height: DEFAULT_TRACK_HEIGHT,
            collapsed: false,
            color: *color,
            selected: false,
            solo: TrackSoloState::NotSoloing,
            first_track_index: index,
            kind: TrackKind::Audio,
            depth: 0,
        };
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .id_salt(("new-track", i)),
            |ui| ghost_header(ui, &track),
        );
    }
}
