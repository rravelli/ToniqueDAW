use egui::{
    Align, Color32, CursorIcon, FontFamily, FontId, Frame, Layout, Panel, Rangef, Rect, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::regular::{
    ARROWS_IN_LINE_VERTICAL, ARROWS_OUT_LINE_VERTICAL, CARET_DOUBLE_LEFT, CARET_DOUBLE_RIGHT, PLUS,
};
use tonique_engine::edit::TrackId;

use crate::{
    config::keymap::Action,
    core::{
        state::{ProjectState, RowTarget, TRACK_NAME},
        track::{DEFAULT_TRACK_HEIGHT, TrackKind, TrackRow, TrackSoloState},
    },
    ui::theme::{ThemeExt, font_size, with_alpha},
    ui::{
        arrangement::track_header::{
            COLOR_BAR_WIDTH, HEADER_INSET, ROW_GAP, TrackHeader, color_bar_x, ghost_header,
        },
        arrangement::{
            row_layout::{Row, RowLayout},
            ruler::RULER_HEIGHT,
        },
        dnd::DragPayload,
        font::PHOSPHOR_REGULAR,
        widget::{context_menu::ContextMenuButton, flat_button::FlatButton, meter::LevelMeter},
        workspace::{BottomView, Workspace},
    },
    utils::display_name,
};

const DEFAULT_WIDTH: f32 = 190.;
const MIN_WIDTH: f32 = 130.;
const MAX_WIDTH: f32 = 480.;
/// Dragging the edge this much narrower than the narrowest collapses the
/// list.
const COLLAPSE_PAST: f32 = 60.;
/// The grip resizing the list, inside its left edge.
const HANDLE_WIDTH: f32 = 5.;
/// Collapsed, the list is a strip of the tracks' colours and meters.
const COLLAPSED_WIDTH: f32 = 26.;
const COLLAPSED_BAR_WIDTH: f32 = 4.;
const COLLAPSED_METER_WIDTH: f32 = 8.;
/// Between the panel's edges and the headers (the left one holds the
/// handle).
const PANEL_MARGIN: f32 = 3.;
/// The buttons in the panel's head.
const HEAD_BUTTON_SIZE: f32 = 22.;
/// How opaque the accent is over a row an effect would be dropped on.
const DROP_FILL_ALPHA: u8 = 40;
const ADD_TRACK_HEIGHT: f32 = 24.;
/// Above the master's header, below the footer's top line.
pub const FOOTER_PADDING: f32 = 3.;

/// Share of a group header's height, from the top, where a drop goes above
/// the group rather than into it.
const ABOVE_GROUP: f32 = 0.3;

/// The track list: a panel on the right of the timeline, resizable and
/// collapsible to a strip of the tracks' colours. A head in line with the
/// ruler, the rows in line with the timeline's lanes (scrolled with them),
/// and the master in a footer in line with the timeline's.
pub struct TrackHeaders {
    /// The row being dragged to a new place.
    dragging: Option<TrackId>,
    /// The track to start renaming, next drawn.
    rename: Option<TrackId>,
    /// The timeline's lanes, as last drawn: an effect dragged over one
    /// lights its header too.
    lanes: Rect,
    /// Where the rows are shown, between the head and the footer.
    rows: Rect,
    /// The head's button to collapse or expand the list was clicked, or
    /// its edge dragged past the narrowest.
    toggle: bool,
    /// Expanded. Kept here, not left to egui's resizing: the timeline,
    /// drawn after the list, would take the outer half of egui's handle.
    width: f32,
    /// The width when the handle's drag started.
    drag_start: Option<f32>,
}

impl TrackHeaders {
    pub fn new() -> Self {
        Self {
            dragging: None,
            rename: None,
            lanes: Rect::NOTHING,
            rows: Rect::NOTHING,
            toggle: false,
            width: DEFAULT_WIDTH,
            drag_start: None,
        }
    }

    /// Start typing a new name for `track`, if it's on screen.
    pub fn rename(&mut self, track: TrackId) {
        self.rename = Some(track);
    }

    /// Where the rows were shown this frame: scrolling over them scrolls
    /// the timeline's lanes too.
    pub fn rows_rect(&self) -> Rect {
        self.rows
    }

    /// The timeline's lanes, once drawn this frame.
    pub fn set_lanes(&mut self, lanes: Rect) {
        self.lanes = lanes;
    }

    /// Show the list, as a panel: before the central panel, which takes
    /// what's left. `new_tracks`: the colours of the tracks a drop on the
    /// timeline would add, shown as ghost headers below the last. Returns
    /// the rows (on screen) of the track an effect would be dropped on,
    /// whose lane the timeline lights.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        new_tracks: &[Color32],
    ) -> Option<Rangef> {
        let frame = Frame::new().fill(ui.app_theme().bg_panel);
        let expanded = Panel::right("track-list")
            .resizable(false)
            .exact_size(self.width)
            .frame(frame);
        let collapsed = Panel::right("track-list-collapsed")
            .resizable(false)
            .exact_size(COLLAPSED_WIDTH)
            .frame(frame);
        let mut open = workspace.track_list_open;
        let target = Panel::show_switched(ui, &mut open, collapsed, expanded, |ui, expanded| {
            self.panel_ui(ui, state, workspace, expanded, new_tracks)
        })
        .inner;
        workspace.track_list_open = open != std::mem::take(&mut self.toggle);
        target
    }

    fn panel_ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        expanded: bool,
        new_tracks: &[Color32],
    ) -> Option<Rangef> {
        let rect = ui.max_rect();
        // Everything is drawn in children of fixed rects, which egui doesn't
        // count: claim the panel, or it shrinks to its smallest.
        ui.expand_to_include_rect(rect);
        let head = Rect::from_x_y_ranges(rect.x_range(), rect.top()..=rect.top() + RULER_HEIGHT);
        let footer_height = (state.master_track().height + FOOTER_PADDING)
            .min(rect.height() - head.height())
            .max(0.);
        let footer_top = rect.bottom() - footer_height;
        self.rows = Rect::from_x_y_ranges(rect.x_range(), head.bottom()..=footer_top);
        let footer = Rect::from_x_y_ranges(rect.x_range(), footer_top..=rect.bottom());
        let layout = RowLayout::new(state, self.rows.top(), state.grid.offset.y);

        let theme = ui.app_theme();
        for y in [head.bottom(), footer.top()] {
            ui.painter()
                .hline(rect.x_range(), y - 0.5, Stroke::new(1., theme.border));
        }
        self.head_ui(ui, state, head, expanded);
        if !expanded {
            self.collapsed_ui(ui, state, &layout, footer);
            return None;
        }
        let target = self.headers_ui(ui, state, workspace, &layout, footer, new_tracks);
        // Over the headers, so it gets the drag where they meet.
        self.resize_handle(ui, rect);
        target
    }

    /// The rows' headers, the ghosts of the tracks a drop would add, and
    /// the master's header in the footer.
    fn headers_ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        layout: &RowLayout,
        footer: Rect,
        new_tracks: &[Color32],
    ) -> Option<Rangef> {
        let rect = ui.max_rect();
        let rename = self.rename.take();
        let x = rect.left() + PANEL_MARGIN..=rect.right() - PANEL_MARGIN;
        let mut rows = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_x_y_ranges(
                    x.clone(),
                    self.rows.top() - state.grid.offset.y..=self.rows.bottom(),
                ))
                .id_salt("track-area"),
        );
        rows.set_clip_rect(self.rows);
        let target = self.track_panel(&mut rows, state, workspace, layout, rename);
        paint_new_track_headers(&mut rows, state, layout, new_tracks);
        // The master, in the footer below the tracks.
        let master_track = state.master_track();
        let mut master = ui.new_child(UiBuilder::new().max_rect(Rect::from_x_y_ranges(
            x,
            footer.top() + FOOTER_PADDING..=footer.bottom(),
        )));
        master.set_clip_rect(footer);
        TrackHeader::new().ui(
            &mut master,
            &master_track,
            state,
            rename == Some(master_track.id),
        );
        target
    }

    /// The grip along the list's left edge: dragging it resizes the list,
    /// past the narrowest collapses it.
    fn resize_handle(&mut self, ui: &mut Ui, rect: Rect) {
        let handle =
            Rect::from_x_y_ranges(rect.left()..=rect.left() + HANDLE_WIDTH, rect.y_range());
        let response = ui
            .interact(handle, ui.id().with("track-list-resize"), Sense::drag())
            .on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);
        if response.drag_started() {
            self.drag_start = Some(self.width);
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let wanted = rect.right() - pointer.x;
            // The timeline keeps at least as much room as the list.
            let max = MAX_WIDTH
                .min(ui.ctx().content_rect().width() / 2.)
                .max(MIN_WIDTH);
            self.width = wanted.clamp(MIN_WIDTH, max);
            if wanted < MIN_WIDTH - COLLAPSE_PAST {
                // Collapsed, at the width it had: expanding finds it again.
                self.width = self.drag_start.unwrap_or(DEFAULT_WIDTH);
                self.toggle = true;
            }
        }
        if response.drag_stopped() {
            self.drag_start = None;
        }
        if response.hovered() || response.dragged() {
            ui.painter().vline(
                rect.left() + 1.,
                rect.y_range(),
                Stroke::new(2., ui.app_theme().accent),
            );
        }
    }

    /// Expand and collapse every row, and collapse the list; or, collapsed,
    /// expand it.
    fn head_ui(&mut self, ui: &mut Ui, state: &mut ProjectState, head: Rect, expanded: bool) {
        let mut head_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(head.shrink2(vec2(PANEL_MARGIN, 0.)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        let ui = &mut head_ui;
        ui.spacing_mut().item_spacing = vec2(2., 0.);
        let button = |icon: &str, tooltip: String| {
            FlatButton::ghost(icon)
                .square(HEAD_BUTTON_SIZE)
                .font(FontId::new(14., FontFamily::Name(PHOSPHOR_REGULAR.into())))
                .tooltip(tooltip)
        };
        let keymap = &state.settings().keymap;
        let (icon, text) = if expanded {
            (CARET_DOUBLE_RIGHT, "Collapse track list")
        } else {
            (CARET_DOUBLE_LEFT, "Expand track list")
        };
        let tooltip = keymap.with_shortcut(ui.ctx(), text, Action::ToggleTrackList);
        if ui.add(button(icon, tooltip)).clicked() {
            self.toggle = true;
        }
        if !expanded {
            return;
        }
        ui.add_space(ui.available_width() - 2. * HEAD_BUTTON_SIZE - 2.);
        if ui
            .add(button(
                ARROWS_IN_LINE_VERTICAL,
                "Collapse all tracks".into(),
            ))
            .clicked()
        {
            state.set_all_collapsed(true);
        }
        if ui
            .add(button(ARROWS_OUT_LINE_VERTICAL, "Expand all tracks".into()))
            .clicked()
        {
            state.set_all_collapsed(false);
        }
    }

    /// The list collapsed: each row a bar of its colour, named on hover;
    /// clicking one selects it.
    fn collapsed_ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        layout: &RowLayout,
        footer: Rect,
    ) {
        let theme = ui.app_theme();
        let master = state.master_track();
        let footer_row = Rangef::new(
            footer.top() + FOOTER_PADDING,
            footer.top() + FOOTER_PADDING + master.height,
        );
        let rows = layout
            .rows()
            .iter()
            .map(|row| (&row.track, row.y, self.rows))
            .chain([(&master, footer_row, footer)]);
        // A bar of the track's colour, and its meter.
        let left =
            ui.max_rect().center().x - (COLLAPSED_BAR_WIDTH + 3. + COLLAPSED_METER_WIDTH) / 2.;
        for (track, y, clip) in rows {
            let bar =
                Rect::from_x_y_ranges(left..=left + COLLAPSED_BAR_WIDTH, y.shrink(HEADER_INSET));
            let meter = Rect::from_x_y_ranges(
                bar.right() + 3.0..=bar.right() + 3. + COLLAPSED_METER_WIDTH,
                bar.y_range(),
            );
            if !bar.intersects(clip) {
                continue;
            }
            let row = Rect::from_x_y_ranges(ui.max_rect().x_range(), y).intersect(clip);
            let response = ui
                .interact(
                    row,
                    ui.id().with(("collapsed-row", track.id)),
                    Sense::click(),
                )
                .on_hover_text(display_name(&track.name, track.first_track_index));
            if response.clicked() {
                state.select_track(&track.id);
            }
            let color = if track.is_silenced() {
                theme.text_disabled
            } else {
                track.color
            };
            let painter = ui.painter().with_clip_rect(clip);
            if track.selected || response.hovered() {
                painter.rect_filled(row, 2., theme.hover_overlay);
            }
            painter.rect_filled(bar, 1., color);
            if let Some(metrics) = state.metrics.tracks.get(&track.id) {
                let mut meter_ui = ui.new_child(UiBuilder::new().max_rect(meter));
                meter_ui.set_clip_rect(clip);
                meter_ui.add(
                    LevelMeter::new(meter.size(), metrics.clone()).disabled(track.is_silenced()),
                );
            }
        }
    }

    /// The rows' headers, and "Add track" below. Returns the rows of the
    /// track an effect would be dropped on.
    fn track_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        layout: &RowLayout,
        rename: Option<TrackId>,
    ) -> Option<Rangef> {
        let viewport = self.rows;
        let mut target = None;
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
                let lane = Rect::from_x_y_ranges(self.lanes.x_range(), row.y);
                if dragging_effect(ui)
                    && ui
                        .input(|i| i.pointer.hover_pos())
                        .is_some_and(|p| lane.contains(p) || response.rect.contains(p))
                {
                    paint_drop_target(ui, response.rect);
                    target = Some(row.y);
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
        target
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

/// A row an effect would be dropped on, lit and outlined in the accent:
/// its header here, its lane by the timeline.
pub fn paint_drop_target(ui: &Ui, rect: Rect) {
    let accent = ui.app_theme().accent;
    let painter = ui.painter();
    painter.rect_filled(rect, 2., with_alpha(accent, DROP_FILL_ALPHA));
    painter.rect_stroke(rect, 2., Stroke::new(1.5, accent), StrokeKind::Inside);
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
