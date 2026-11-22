//! Painting a [`Scene`]: the lanes and the grid, the clips over them, the
//! master's footer, the ruler, then the edit cursor and the playhead over everything.

use gpui::{Bounds, ContentMask, Pixels, Window, px};

use crate::{
    core::track::DEFAULT_TRACK_HEIGHT,
    geometry::Rect,
    theme::{Color, hsla, mix, mono, size, with_alpha},
    timeline::{
        BARS_HEIGHT, DROP_FILL_ALPHA, DROP_OUTLINE, LOOP_HEIGHT, ROW_GAP, RULER_HEIGHT,
        clip::{ClipPaint, paint_clip},
        fades::paint::{paint_handles, paint_label, paint_seams},
        footer::paint_footer,
        ruler::{paint_loop, paint_playhead},
        scene::Scene,
    },
    wave::WaveCache,
    widgets::paint_text,
};

/// How strongly a selected track's colour tints its lane.
const LANE_TINT: f32 = 0.06;
/// How opaque the lane of a track a drop would add is.
const NEW_LANE_ALPHA: u8 = 24;
const EDIT_CURSOR_ALPHA: u8 = 120;
/// Width of the line showing where playback starts, on selected lanes.
const START_LINE_WIDTH: f32 = 1.5;
/// How opaque the line where a click on a clip's body places the edit
/// cursor is.
const CURSOR_HINT_ALPHA: u8 = 170;
/// A selection zone's outline: dashes, and the gaps between them.
const DASH: f32 = 5.;
const DASH_GAP: f32 = 3.;

/// The timeline's bounds, and painting in them.
pub(super) struct Canvas {
    left: f32,
    top: f32,
    pub(super) width: f32,
    height: f32,
    /// The footer's height, below the lanes.
    footer: f32,
}

impl Canvas {
    /// A rect local to the timeline, in the window.
    pub(super) fn rect(&self, left: f32, top: f32, width: f32, height: f32) -> Rect {
        Rect::new(self.left + left, self.top + top, width, height)
    }

    /// Between the ruler and the footer.
    pub(super) fn lanes(&self) -> Rect {
        let height = (self.height - RULER_HEIGHT - self.footer).max(0.);
        self.rect(0., RULER_HEIGHT, self.width, height)
    }

    fn footer(&self) -> Rect {
        let lanes = self.lanes();
        Rect::new(
            lanes.left,
            lanes.bottom(),
            self.width,
            self.top + self.height - lanes.bottom(),
        )
    }

    fn fill(&self, window: &mut Window, rect: Rect, color: Color) {
        window.paint_quad(gpui::fill(rect.bounds(), hsla(color)));
    }
}

pub fn paint(
    scene: &Scene,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    context: &mut gpui::App,
    waves: &mut WaveCache,
) {
    let canvas = Canvas {
        left: f32::from(bounds.origin.x),
        top: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        footer: scene.footer_height,
    };
    paint_lanes(scene, &canvas, window);
    paint_zone_veil(scene, &canvas, window);
    paint_clips(scene, &canvas, window, context, waves);
    paint_over_clips(scene, &canvas, window);
    paint_footer(window, context, canvas.footer(), &scene.scope, &scene.theme);
    paint_effect_drop(scene, &canvas, window);
    paint_ruler(scene, &canvas, window, context);
    paint_cursors(scene, &canvas, window);
    paint_label(
        window,
        context,
        &scene.fades,
        &scene.theme,
        (canvas.left, canvas.top),
    );
}

/// The lanes, the gaps between them, the loop and the grid over them.
fn paint_lanes(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let theme = &scene.theme;
    let lanes = canvas.lanes();
    canvas.fill(
        window,
        canvas.rect(0., 0., canvas.width, canvas.height),
        theme.bg_base,
    );
    for row in &scene.rows {
        let lane = canvas
            .rect(0., row.top, canvas.width, row.height)
            .intersect(lanes);
        if row.selected && !lane.is_empty() {
            canvas.fill(window, lane, mix(theme.bg_base, row.color, LANE_TINT));
        }
        let gap = canvas
            .rect(0., row.top + row.height, canvas.width, ROW_GAP)
            .intersect(lanes);
        if !gap.is_empty() {
            canvas.fill(window, gap, theme.separator);
        }
    }
    for &(top, color) in &scene.new_lanes {
        let lane = canvas
            .rect(0., top, canvas.width, DEFAULT_TRACK_HEIGHT)
            .intersect(lanes);
        canvas.fill(window, lane, with_alpha(color, NEW_LANE_ALPHA));
        let gap = canvas
            .rect(0., top + DEFAULT_TRACK_HEIGHT, canvas.width, ROW_GAP)
            .intersect(lanes);
        canvas.fill(window, gap, theme.separator);
    }
    if scene.looping {
        let (loop_start, loop_end) = scene.loop_x;
        let region = canvas
            .rect(
                loop_start,
                RULER_HEIGHT,
                loop_end - loop_start,
                lanes.height,
            )
            .intersect(lanes);
        canvas.fill(window, region, theme.loop_region);
    }
    for &(mark_x, bar) in &scene.marks.lines {
        let color = if bar { theme.grid_bar } else { theme.grid_beat };
        canvas.fill(
            window,
            canvas.rect(mark_x.round(), RULER_HEIGHT, 1., lanes.height),
            color,
        );
    }
}

/// The clips, and the one a drop would add, clipped to the lanes.
fn paint_clips(
    scene: &Scene,
    canvas: &Canvas,
    window: &mut Window,
    context: &mut gpui::App,
    waves: &mut WaveCache,
) {
    let lanes = canvas.lanes();
    let mask = ContentMask {
        bounds: lanes.bounds(),
    };
    window.with_content_mask(Some(mask), |window| {
        let mut painter = ClipPaint {
            window,
            context,
            waves,
            theme: &scene.theme,
            lanes,
            origin: (canvas.left, canvas.top),
            pixels_per_beat: scene.pixels_per_beat,
            bpm: scene.bpm,
        };
        for (bar, color) in &scene.overviews {
            let bar = Rect::new(
                canvas.left + bar.left,
                canvas.top + bar.top,
                bar.width,
                bar.height,
            );
            painter
                .window
                .paint_quad(gpui::fill(bar.bounds(), hsla(*color)).corner_radii(px(1.)));
        }
        for clip in &scene.clips {
            paint_clip(&mut painter, clip, false);
        }
        let origin = (canvas.left, canvas.top);
        paint_seams(painter.window, &scene.fades, &scene.theme, origin, lanes);
        paint_handles(painter.window, &scene.fades, &scene.theme, origin, lanes);
        if let Some(clip) = &scene.drop {
            paint_clip(&mut painter, clip, true);
        }
    });
}

/// The selection zone's veil, under the clips: they light their own part
/// of it.
fn paint_zone_veil(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let Some(zone) = scene.zone else { return };
    let mask = ContentMask {
        bounds: canvas.lanes().bounds(),
    };
    let zone = canvas.rect(zone.left, zone.top, zone.width, zone.height);
    let fill = hsla(scene.theme.selection_fill);
    window.with_content_mask(Some(mask), |window| {
        window.paint_quad(gpui::fill(zone.bounds(), fill).corner_radii(px(2.)));
    });
}

/// Over the clips, in the lanes: the lit trim handle, where playback
/// starts on the selected lanes, where a click would place the edit
/// cursor, what dragged clips snapped to, and the zone's outline.
fn paint_over_clips(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let theme = &scene.theme;
    let lanes = canvas.lanes();
    let mask = ContentMask {
        bounds: lanes.bounds(),
    };
    window.with_content_mask(Some(mask), |window| {
        if let Some(handle) = scene.lit_handle {
            let handle = canvas.rect(handle.left, handle.top, handle.width, handle.height);
            window.paint_quad(
                gpui::fill(handle.bounds(), hsla(theme.hover_overlay)).corner_radii(px(1.)),
            );
        }
        for row in scene.rows.iter().filter(|row| row.selected) {
            let line = canvas.rect(
                scene.edit_cursor - START_LINE_WIDTH / 2.,
                row.top,
                START_LINE_WIDTH,
                row.height,
            );
            canvas.fill(window, line, theme.edit_cursor);
        }
        if let Some((hint_x, body)) = scene.cursor_hint {
            let line = canvas.rect(hint_x.round(), body.top, 1., body.height);
            canvas.fill(window, line, with_alpha(theme.text, CURSOR_HINT_ALPHA));
        }
        if let Some(snap_x) = scene.snap_line {
            let line = canvas.rect(snap_x.round(), RULER_HEIGHT, 1., lanes.height);
            canvas.fill(window, line, theme.text_muted);
        }
        if let Some(zone) = scene.zone {
            // Only what's on screen: zoomed in, a zone can be millions of
            // pixels wide. Edges cut off land just outside.
            let wide = Rect::new(
                lanes.left - 2.,
                lanes.top - 2.,
                lanes.width + 4.,
                lanes.height + 4.,
            );
            let edge = canvas
                .rect(zone.left, zone.top, zone.width, zone.height)
                .intersect(wide);
            paint_dashed_outline(window, edge, theme.selection_stroke);
        }
    });
}

/// A dashed 1 pixel outline just inside `rect`: reads as an area being
/// drawn, not as something selected.
fn paint_dashed_outline(window: &mut Window, rect: Rect, color: Color) {
    if rect.is_empty() {
        return;
    }
    let color = hsla(color);
    let mut dash = |left: f32, top: f32, width: f32, height: f32| {
        window.paint_quad(gpui::fill(
            Rect::new(left, top, width, height).bounds(),
            color,
        ));
    };
    let (right, bottom) = (rect.right() - 1., rect.bottom() - 1.);
    let mut along = rect.left;
    while along < rect.right() {
        let length = DASH.min(rect.right() - along);
        dash(along, rect.top, length, 1.);
        dash(along, bottom, length, 1.);
        along += DASH + DASH_GAP;
    }
    let mut down = rect.top;
    while down < rect.bottom() {
        let length = DASH.min(rect.bottom() - down);
        dash(rect.left, down, 1., length);
        dash(right, down, 1., length);
        down += DASH + DASH_GAP;
    }
}

/// The lane an effect would be dropped on, lit and outlined as its
/// header: under the ruler, over the footer for the master.
fn paint_effect_drop(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let Some((top, height)) = scene.effect_drop else {
        return;
    };
    let accent = scene.theme.accent;
    let below_ruler = canvas.rect(0., RULER_HEIGHT, canvas.width, canvas.height - RULER_HEIGHT);
    let mask = ContentMask {
        bounds: below_ruler.bounds(),
    };
    let lane = canvas.rect(0., top, canvas.width, height);
    window.with_content_mask(Some(mask), |window| {
        window.paint_quad(
            gpui::fill(lane.bounds(), hsla(with_alpha(accent, DROP_FILL_ALPHA)))
                .corner_radii(px(2.))
                .border_widths(px(DROP_OUTLINE))
                .border_color(hsla(accent)),
        );
    });
}

/// The loop lane, the bars with their labels, and the times.
fn paint_ruler(scene: &Scene, canvas: &Canvas, window: &mut Window, context: &mut gpui::App) {
    let theme = &scene.theme;
    canvas.fill(
        window,
        canvas.rect(0., 0., canvas.width, RULER_HEIGHT),
        theme.bg_panel,
    );
    canvas.fill(
        window,
        canvas.rect(0., RULER_HEIGHT - 1., canvas.width, 1.),
        theme.separator,
    );
    paint_loop(scene, canvas, window);
    for (mark_x, text, bar) in &scene.marks.labels {
        // Bars' ticks run the lane's height; the others' are short.
        let top = if *bar { 2. } else { 9. };
        canvas.fill(
            window,
            canvas.rect(mark_x.round(), LOOP_HEIGHT + top, 1., BARS_HEIGHT - top),
            theme.border,
        );
        let color = if *bar { theme.text } else { theme.text_muted };
        let (text_left, text_top) = (canvas.left + mark_x + 4., canvas.top + LOOP_HEIGHT + 3.);
        paint_text(
            window,
            context,
            text.clone(),
            text_left,
            text_top,
            size::LABEL,
            hsla(color),
            mono(),
        );
    }
    let times_top = LOOP_HEIGHT + BARS_HEIGHT;
    for (mark_x, text) in &scene.marks.times {
        canvas.fill(
            window,
            canvas.rect(
                mark_x.round(),
                times_top + 8.,
                1.,
                RULER_HEIGHT - times_top - 8.,
            ),
            theme.separator,
        );
        let (text_left, text_top) = (canvas.left + mark_x + 4., canvas.top + times_top + 2.);
        paint_text(
            window,
            context,
            text.clone(),
            text_left,
            text_top,
            size::SMALL,
            hsla(theme.text_disabled),
            mono(),
        );
    }
}

/// The edit cursor, then the playhead over it, with its head on the ruler:
/// down to the footer.
fn paint_cursors(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let theme = &scene.theme;
    let lanes = canvas.lanes();
    if (scene.edit_cursor - scene.playhead).abs() >= 1. {
        let line = canvas.rect(scene.edit_cursor.round(), RULER_HEIGHT, 1., lanes.height);
        canvas.fill(
            window,
            line,
            with_alpha(theme.edit_cursor, EDIT_CURSOR_ALPHA),
        );
    }
    paint_playhead(scene, canvas, window);
}
