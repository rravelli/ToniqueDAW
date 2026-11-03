//! A clip's fades and crossfades on the timeline: their curves over its
//! waveform, the faded part shaded; a handle at the top of each, shown over
//! the clip, to drag its length; the curve itself to drag its curvature; a
//! menu on either for its shape.

use egui::{
    Color32, CursorIcon, Id, Mesh, Painter, Popup, Pos2, Rect, Response, Sense, Shape, Stroke, Ui,
    Vec2, pos2, vec2,
};
use egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE;
use tonique_engine::{
    edit::{ClipFades, Fade},
    nodes::{FadeCurve, FadeShape},
};

use crate::{
    core::{
        clip::{AudioClip, Crossfades, Seam},
        state::ProjectState,
    },
    ui::{
        arrangement::clip::{ClipView, HEADER_HEIGHT, TRIM_HANDLE_WIDTH},
        theme::{ThemeExt, with_alpha},
        widget::context_menu::ContextMenuButton,
    },
};

/// A fade's handle, a square at the top of the clip's body.
const HANDLE: f32 = 8.;
/// Around it, what can be grabbed.
const HANDLE_REACH: f32 = 2.;
/// How near a curve grabs it.
const CURVE_REACH: f32 = 5.;
/// Too small a clip has no handles: its trim handles take it all.
const MIN_WIDTH: f32 = 2. * (TRIM_HANDLE_WIDTH + HANDLE + HANDLE_REACH);
/// A fade narrower than this has no curve to grab.
const MIN_CURVE_WIDTH: f32 = 8.;
/// Between two points of a curve drawn.
const CURVE_STEP: f32 = 2.;
const SHADE_ALPHA: u8 = 130;
const CROSSFADE_SHADE_ALPHA: u8 = 70;
/// A crossfade made by double-clicking a seam, in seconds.
const DEFAULT_CROSSFADE_S: f64 = 0.1;
const CURVE_WIDTH: f32 = 1.;
const HOT_CURVE_WIDTH: f32 = 2.;

/// Which end of a clip a fade is at.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FadeEnd {
    In,
    Out,
}

impl FadeEnd {
    fn heard(self, clip: &AudioClip, crossed: Crossfades, bpm: f32) -> Fade {
        let (fade_in, fade_out) = clip.heard_fades(crossed, bpm);
        match self {
            Self::In => fade_in,
            Self::Out => fade_out,
        }
    }

    fn crossed(self, crossed: Crossfades) -> bool {
        match self {
            Self::In => crossed.start.is_some(),
            Self::Out => crossed.end.is_some(),
        }
    }

    fn at_seam(self, crossed: Crossfades) -> bool {
        match self {
            Self::In => crossed.start_seam,
            Self::Out => crossed.end_seam,
        }
    }

    fn curve_mut(self, fades: &mut ClipFades) -> &mut FadeCurve {
        match self {
            Self::In => &mut fades.fade_in.curve,
            Self::Out => &mut fades.fade_out.curve,
        }
    }
}

/// A fade's part of the timeline, and its gain over it.
#[derive(Clone, Copy)]
struct Span {
    left: f32,
    right: f32,
    curve: FadeCurve,
    /// Rises from left to right; otherwise it's a fade out, played backwards
    /// from the right.
    rising: bool,
}

impl Span {
    fn gain(&self, x: f32) -> f32 {
        let width = (self.right - self.left).max(f32::EPSILON);
        let t = if self.rising {
            x - self.left
        } else {
            self.right - x
        };
        self.curve.gain(t / width)
    }

    /// Its points over `body`, as far as `painter` shows it.
    fn points(&self, painter: &Painter, body: Rect) -> Vec<Pos2> {
        let shown = painter.clip_rect();
        let (from, to) = (self.left.max(shown.left()), self.right.min(shown.right()));
        if from >= to {
            return Vec::new();
        }
        let steps = ((to - from) / CURVE_STEP).ceil().max(1.) as usize;
        (0..=steps)
            .map(|k| {
                let x = from + (to - from) * k as f32 / steps as f32;
                pos2(x, gain_y(body, self.gain(x)))
            })
            .collect()
    }

    fn line(&self, painter: &Painter, body: Rect, stroke: Stroke) {
        let points = self.points(painter, body);
        if points.len() > 1 {
            painter.add(Shape::line(points, stroke));
        }
    }

    /// Whatever the fade takes away: all that's above its curve.
    fn shade(&self, painter: &Painter, body: Rect, color: Color32) {
        let mut mesh = Mesh::default();
        for (k, point) in self.points(painter, body).into_iter().enumerate() {
            mesh.colored_vertex(pos2(point.x, body.top()), color);
            mesh.colored_vertex(point, color);
            if k > 0 {
                let k = k as u32 * 2;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k - 1, k, k + 1);
            }
        }
        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
    }

    /// Whether `pointer` is on the curve, inside `area`.
    fn is_near(&self, pointer: Pos2, area: Rect, body: Rect) -> bool {
        (self.left..=self.right).contains(&pointer.x)
            && area.contains(pointer)
            && (pointer.y - gain_y(body, self.gain(pointer.x))).abs() <= CURVE_REACH
    }
}

fn gain_y(body: Rect, gain: f32) -> f32 {
    body.bottom() - gain * body.height()
}

/// `seconds` as read in a tooltip.
fn format_length(seconds: f64) -> String {
    if seconds < 1. {
        format!("{:.0} ms", seconds * 1000.)
    } else {
        format!("{seconds:.2} s")
    }
}

pub fn shape_name(shape: FadeShape) -> &'static str {
    match shape {
        FadeShape::Linear => "Linear",
        FadeShape::EqualPower => "Equal power",
        FadeShape::Exponential => "Exponential",
    }
}

/// What's picked on a fade's menu.
enum MenuChoice {
    Shape(FadeShape),
    Reset,
}

/// The shapes to pick for `curve`, and to reset it. Only on a right-click
/// of `response`.
fn fade_menu(response: &Response, curve: FadeCurve) -> Option<MenuChoice> {
    let mut choice = None;
    response.context_menu(|ui| {
        ui.vertical(|ui| {
            for shape in FadeShape::ALL {
                let button = ContextMenuButton::new("", shape_name(shape));
                if ui.add(button.checked(curve.shape == shape)).clicked() {
                    choice = Some(MenuChoice::Shape(shape));
                }
            }
            let reset = ContextMenuButton::new(ARROW_COUNTER_CLOCKWISE, "Reset");
            if ui.add(reset).clicked() {
                choice = Some(MenuChoice::Reset);
            }
        });
    });
    choice
}

/// Whether the widget `id` is in the middle of something (or just finished),
/// wherever the pointer is: it must stay for that to go on, and to be told.
fn engaged(ui: &Ui, id: Id) -> bool {
    ui.ctx().is_being_dragged(id)
        || ui.ctx().drag_stopped_id() == Some(id)
        || Popup::is_id_open(ui.ctx(), id.with("popup"))
}

/// The curvature `curve` gets dragged by `response` over `body`: up bends it
/// up.
fn dragged_curvature(response: &Response, curve: FadeCurve, body: Rect) -> FadeCurve {
    let by = -2. * response.drag_delta().y / body.height().max(1.);
    curve.with_curvature(curve.curvature + by)
}

impl ClipView<'_> {
    /// Where the fades are drawn: the clip's body.
    fn fades_area(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.rect.left(), self.rect.top() + HEADER_HEIGHT),
            self.rect.max,
        )
    }

    fn span(&self, end: FadeEnd, fade: Fade, pixels_per_second: f32) -> Span {
        let width = fade.length_s as f32 * pixels_per_second;
        let (left, right) = match end {
            FadeEnd::In => (self.rect.left(), self.rect.left() + width),
            FadeEnd::Out => (self.rect.right() - width, self.rect.right()),
        };
        Span {
            left,
            right,
            curve: fade.curve,
            rising: end == FadeEnd::In,
        }
    }

    /// The clip's fades over what's drawn of it, on `painter`, in `ink`.
    pub(super) fn paint_fades(&self, painter: &Painter, state: &ProjectState, ink: Color32) {
        let body = self.fades_area();
        if body.height() < 2. {
            return;
        }
        let bpm = state.bpm();
        let per_second = self.rect.width() / self.clip.seconds(bpm).max(1e-6) as f32;
        let shade = with_alpha(painter.ctx().app_theme().bg_base, SHADE_ALPHA);
        for end in [FadeEnd::In, FadeEnd::Out] {
            let fade = end.heard(self.clip, self.crossfades, bpm);
            if end.crossed(self.crossfades) || fade.length_s <= 0. {
                continue;
            }
            let span = self.span(end, fade, per_second);
            span.shade(painter, body, shade);
            span.line(painter, body, Stroke::new(CURVE_WIDTH, ink));
        }
    }

    /// The handles and curves of the clip's fades, to drag; applied to
    /// `state`. Returns whether the pointer is on one.
    pub(super) fn fades_ui(&self, ui: &mut Ui, state: &mut ProjectState) -> bool {
        let body = self.fades_area();
        if self.collapsed || body.height() < 2. * HANDLE || self.rect.width() < MIN_WIDTH {
            return false;
        }
        let mut hot = false;
        for end in [FadeEnd::In, FadeEnd::Out] {
            // A seam has its own handle: the fade's only there to get hold
            // of one there already.
            let fade = end.heard(self.clip, self.crossfades, state.bpm());
            let in_the_way = end.at_seam(self.crossfades) && fade.length_s <= 0.;
            if !end.crossed(self.crossfades) && !in_the_way {
                hot |= self.fade_ui(ui, state, end, body);
            }
        }
        hot
    }

    fn fade_ui(&self, ui: &mut Ui, state: &mut ProjectState, end: FadeEnd, body: Rect) -> bool {
        let (clip, viewport) = (self.clip, self.viewport);
        let bpm = state.bpm();
        let fade = end.heard(clip, self.crossfades, bpm);
        let per_second = self.rect.width() / clip.seconds(bpm).max(1e-6) as f32;
        let span = self.span(end, fade, per_second);
        let theme = ui.app_theme();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let idle = ui.ctx().dragged_id().is_none();
        let ink = theme.text;
        let mut hot = false;

        // Under the handle, which it meets at the top of the clip.
        let curve_id = Id::new(("clip-fade-curve", clip.id, end as u8));
        let area = body
            .shrink2(vec2(TRIM_HANDLE_WIDTH, 0.))
            .intersect(viewport);
        let on_curve = pointer.filter(|p| idle && span.is_near(*p, area, body));
        if span.right - span.left >= MIN_CURVE_WIDTH
            && (on_curve.is_some() || engaged(ui, curve_id))
        {
            let at = pointer.unwrap_or(area.center());
            let rect = Rect::from_center_size(at, Vec2::splat(2. * CURVE_REACH));
            let response = ui
                .interact(rect.intersect(viewport), curve_id, Sense::click_and_drag())
                .on_hover_and_drag_cursor(CursorIcon::ResizeVertical);
            hot |= response.hovered() || response.dragged();
            if hot {
                let painter = ui.painter_at(body.intersect(viewport));
                span.line(&painter, body, Stroke::new(HOT_CURVE_WIDTH, ink));
            }
            if response.dragged() {
                let mut edited = clip.clone();
                let curve = end.curve_mut(&mut edited.fades);
                *curve = dragged_curvature(&response, *curve, body);
                let curvature = curve.curvature;
                state.resize_clip(edited);
                response.show_tooltip_text(format!("{:+.0}%", curvature * 100.));
            }
            if response.drag_stopped() {
                state.commit_resize_clip(clip.clone());
            }
            self.apply_fade_menu(&response, state, end, fade.curve);
        }

        let handle_id = Id::new(("clip-fade", clip.id, end as u8));
        let over_clip = idle && ui.rect_contains_pointer(self.rect.intersect(viewport));
        if over_clip || engaged(ui, handle_id) {
            let inset = TRIM_HANDLE_WIDTH + HANDLE / 2. + HANDLE_REACH;
            let x = match end {
                FadeEnd::In => span.right,
                FadeEnd::Out => span.left,
            }
            .clamp(self.rect.left() + inset, self.rect.right() - inset);
            let center = pos2(x, body.top() + HANDLE / 2. + HANDLE_REACH);
            let square = Rect::from_center_size(center, Vec2::splat(HANDLE));
            let rect = square.expand(HANDLE_REACH).intersect(viewport);
            let response = ui
                .interact(rect, handle_id, Sense::click_and_drag())
                .on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);
            hot |= response.hovered() || response.dragged();
            let painter = ui.painter_at(viewport);
            painter.rect_filled(square, 1., ink);
            painter.rect_stroke(
                square,
                1.,
                Stroke::new(1., theme.bg_base),
                egui::StrokeKind::Outside,
            );
            if response.dragged()
                && let Some(mouse) = ui.input(|i| i.pointer.interact_pos())
            {
                self.drag_handle(ui, state, &response, end, mouse);
            }
            if response.drag_stopped() {
                state.commit_resize_clip(clip.clone());
            }
            self.apply_fade_menu(&response, state, end, fade.curve);
        }
        hot
    }

    /// Set the length of the fade at `end` to reach `mouse`: to the grid,
    /// unless Alt is held.
    fn drag_handle(
        &self,
        ui: &Ui,
        state: &mut ProjectState,
        response: &Response,
        end: FadeEnd,
        mouse: Pos2,
    ) {
        let (clip, bpm) = (self.clip, state.bpm());
        let beats = state.grid.x_to_beats(mouse.x, self.viewport);
        let beats = if ui.input(|i| i.modifiers.alt) {
            beats
        } else {
            state.grid.snap_at_grid(beats)
        };
        let per_beat = 60. / bpm as f64;
        let mut edited = clip.clone();
        let seconds = match end {
            FadeEnd::In => {
                edited.set_fade_in((beats - clip.position) * per_beat, self.crossfades, bpm);
                edited.fades.fade_in.length_s
            }
            FadeEnd::Out => {
                edited.set_fade_out((clip.end(bpm) - beats) * per_beat, self.crossfades, bpm);
                edited.fades.fade_out.length_s
            }
        };
        state.resize_clip(edited);
        response.show_tooltip_text(format_length(seconds));
    }

    fn apply_fade_menu(
        &self,
        response: &Response,
        state: &mut ProjectState,
        end: FadeEnd,
        curve: FadeCurve,
    ) {
        let Some(choice) = fade_menu(response, curve) else {
            return;
        };
        let mut edited = self.clip.clone();
        match (choice, end) {
            (MenuChoice::Shape(shape), _) => end.curve_mut(&mut edited.fades).shape = shape,
            (MenuChoice::Reset, FadeEnd::In) => edited.fades.fade_in = Fade::default(),
            (MenuChoice::Reset, FadeEnd::Out) => edited.fades.fade_out = Fade::default(),
        }
        state.commit_resize_clip(edited);
    }
}

/// Where two clips of a track touch: a handle on it, shown over either, to
/// drag how far a crossfade reaches either side; once there's one, its
/// curves over both clips, dragged and given a shape as a fade's. It's the
/// later clip's.
pub struct SeamView<'a> {
    pub seam: Seam,
    pub later: &'a AudioClip,
    /// The two clips on screen.
    pub area: Rect,
    /// Its lane's top and height.
    pub top: f32,
    pub height: f32,
    pub viewport: Rect,
    pub ink: Color32,
    pub collapsed: bool,
}

impl SeamView<'_> {
    pub fn ui(&self, ui: &mut Ui, state: &mut ProjectState) {
        let (seam, viewport) = (self.seam, self.viewport);
        let bpm = state.bpm();
        let per_second = state.grid.pixels_per_beat() * bpm / 60.;
        let x = state.grid.beats_to_x(seam.at, viewport);
        let left = x - seam.before_s as f32 * per_second;
        let right = x + seam.after_s as f32 * per_second;
        let top = self.top + HEADER_HEIGHT;
        let lane = Rect::from_x_y_ranges(left..=right, top..=self.top + self.height);
        let theme = ui.app_theme();
        let rising = Span {
            left,
            right,
            curve: seam.curve,
            rising: true,
        };
        let falling = Span {
            left,
            right,
            curve: seam.curve.crossing(),
            rising: false,
        };
        let crossing = seam.length_s() > 0. && lane.is_positive();
        let painter = ui.painter_at(lane.intersect(viewport));
        if crossing && lane.intersects(viewport) {
            painter.rect_filled(lane, 0., with_alpha(theme.bg_base, CROSSFADE_SHADE_ALPHA));
            let stroke = Stroke::new(CURVE_WIDTH, self.ink);
            rising.line(&painter, lane, stroke);
            falling.line(&painter, lane, stroke);
        }
        if self.collapsed || lane_too_small(self.height) {
            return;
        }
        if crossing {
            self.curve_ui(ui, state, rising, falling, lane, x, &painter);
        }
        self.handle_ui(ui, state, x, left, right, top);
    }

    /// The crossfade's curves, to drag or open its menu. Not over the trim
    /// handles, around the seam where they cross.
    #[allow(clippy::too_many_arguments)]
    fn curve_ui(
        &self,
        ui: &mut Ui,
        state: &mut ProjectState,
        rising: Span,
        falling: Span,
        lane: Rect,
        x: f32,
        painter: &Painter,
    ) {
        let (later, viewport) = (self.later, self.viewport);
        let id = Id::new(("clip-crossfade", later.id));
        let curve = later.fades.crossfade.curve;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let idle = ui.ctx().dragged_id().is_none();
        let area = lane.intersect(viewport);
        let on_curve = pointer.filter(|p| {
            idle && (p.x - x).abs() > TRIM_HANDLE_WIDTH
                && (rising.is_near(*p, area, lane) || falling.is_near(*p, area, lane))
        });
        if on_curve.is_none() && !engaged(ui, id) {
            return;
        }
        let at = pointer.unwrap_or(area.center());
        let rect = Rect::from_center_size(at, Vec2::splat(2. * CURVE_REACH));
        let response = ui
            .interact(rect.intersect(viewport), id, Sense::click_and_drag())
            .on_hover_and_drag_cursor(CursorIcon::ResizeVertical);
        if response.hovered() || response.dragged() {
            let hot = Stroke::new(HOT_CURVE_WIDTH, self.ink);
            rising.line(painter, lane, hot);
            falling.line(painter, lane, hot);
        }
        if response.dragged() {
            let mut edited = later.clone();
            edited.fades.crossfade.curve = dragged_curvature(&response, curve, lane);
            let curvature = edited.fades.crossfade.curve.curvature;
            state.resize_clip(edited);
            response.show_tooltip_text(format!("{:+.0}%", curvature * 100.));
        }
        if response.drag_stopped() {
            state.commit_resize_clip(later.clone());
        }
        self.apply_menu(&response, state);
    }

    /// The handle: at the seam, or at the far end of the crossfade, to drag
    /// how far it reaches (to the grid, unless Alt is held). Double-clicked
    /// with no crossfade, it makes one of [`DEFAULT_CROSSFADE_S`].
    fn handle_ui(
        &self,
        ui: &mut Ui,
        state: &mut ProjectState,
        x: f32,
        left: f32,
        right: f32,
        top: f32,
    ) {
        let (seam, later, viewport) = (self.seam, self.later, self.viewport);
        let id = Id::new(("clip-seam", later.id));
        let idle = ui.ctx().dragged_id().is_none();
        let over_clips = idle && ui.rect_contains_pointer(self.area.intersect(viewport));
        if !over_clips && !engaged(ui, id) {
            return;
        }
        let handle_x = if seam.length_s() <= 0. {
            x
        } else if seam.after_s >= seam.before_s {
            right
        } else {
            left
        };
        let center = pos2(handle_x, top + HANDLE / 2. + HANDLE_REACH);
        let square = Rect::from_center_size(center, Vec2::splat(HANDLE));
        let rect = square.expand(HANDLE_REACH).intersect(viewport);
        let response = ui
            .interact(rect, id, Sense::click_and_drag())
            .on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);
        let theme = ui.app_theme();
        let painter = ui.painter_at(viewport);
        painter.rect_filled(square, 1., theme.text);
        painter.rect_stroke(
            square,
            1.,
            Stroke::new(1., theme.bg_base),
            egui::StrokeKind::Outside,
        );
        if response.dragged()
            && let Some(mouse) = ui.input(|i| i.pointer.interact_pos())
        {
            let beats = state.grid.x_to_beats(mouse.x, viewport);
            let beats = if ui.input(|i| i.modifiers.alt) {
                beats
            } else {
                state.grid.snap_at_grid(beats)
            };
            let distance = (beats - seam.at).abs() * 60. / state.bpm() as f64;
            let length = seam.room.length_reaching(distance);
            let mut edited = later.clone();
            edited.fades.crossfade.length_s = length;
            state.resize_clip(edited);
            if seam.room.length_reaching(f64::INFINITY) <= 0. {
                response.show_tooltip_text("No trimmed audio to crossfade");
            } else {
                response.show_tooltip_text(format_length(length));
            }
        }
        if response.drag_stopped() {
            state.commit_resize_clip(later.clone());
        }
        if response.double_clicked() && seam.length_s() <= 0. {
            let (before, after) = seam.room.window(DEFAULT_CROSSFADE_S);
            let mut edited = later.clone();
            edited.fades.crossfade.length_s = before + after;
            state.commit_resize_clip(edited);
        }
        self.apply_menu(&response, state);
    }

    fn apply_menu(&self, response: &Response, state: &mut ProjectState) {
        let later = self.later;
        let Some(choice) = fade_menu(response, later.fades.crossfade.curve) else {
            return;
        };
        let mut edited = later.clone();
        match choice {
            MenuChoice::Shape(shape) => edited.fades.crossfade.curve.shape = shape,
            MenuChoice::Reset => edited.fades.crossfade = ClipFades::default().crossfade,
        }
        state.commit_resize_clip(edited);
    }
}

fn lane_too_small(height: f32) -> bool {
    height - HEADER_HEIGHT < 2. * HANDLE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analysis::AudioInfo,
        core::clip::{clip_seams, crossfades},
        ui::theme::Theme,
    };
    use egui_kittest::{Harness, kittest::Queryable};
    use tonique_engine::{
        edit::ClipId,
        engine::{Engine, EngineConfig},
        time::BeatPos,
    };

    const VIEW: Rect = Rect::from_min_max(Pos2::ZERO, pos2(1000., 200.));
    const TOP_OF_BODY: f32 = HEADER_HEIGHT;
    const BOTTOM: f32 = 96.;

    struct Setup {
        state: ProjectState,
        early: ClipId,
        late: ClipId,
    }

    /// A 4 s file cut in two 4-beat clips, 0 to 4 and 4 to 8 at 120 bpm:
    /// each has the other's two seconds trimmed off.
    fn setup() -> Setup {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        state.grid.fit(BeatPos(0.), BeatPos(10.), VIEW.width());
        let track = state.add_track();
        let early = state.new_clip_id();
        let clip = AudioClip::new(early, AudioInfo::silent(4.), BeatPos(0.), 120.);
        state.add_clips(&track, vec![clip]);
        state.cut_clip_at(&track, BeatPos(4.));
        let late = state
            .clip_views_of(track)
            .iter()
            .find(|c| c.id != early)
            .unwrap()
            .id;
        Setup { state, early, late }
    }

    fn with_crossfade(seconds: f64) -> Setup {
        let mut setup = setup();
        let mut edited = setup.state.find_clip(setup.late).unwrap().1;
        edited.fades.crossfade.length_s = seconds;
        setup.state.commit_resize_clip(edited);
        setup
    }

    fn show(ui: &mut Ui, setup: &mut Setup) {
        let state = &mut setup.state;
        let shown = |state: &ProjectState, id| state.shown_clip(&state.find_clip(id).unwrap().1);
        let (early, late) = (shown(state, setup.early), shown(state, setup.late));
        let bpm = state.bpm();
        let rect = |state: &ProjectState, clip: &AudioClip| {
            Rect::from_min_size(
                pos2(state.grid.beats_to_x(clip.position, VIEW), 0.),
                vec2(state.grid.beats_to_width(clip.length), BOTTOM),
            )
        };
        let seams = clip_seams(&[early.clone(), late.clone()], bpm);
        let found = crossfades(&seams);
        let (early_rect, late_rect) = (rect(state, &early), rect(state, &late));
        for (clip, rect) in [(&early, early_rect), (&late, late_rect)] {
            ClipView {
                clip,
                rect,
                viewport: VIEW,
                color: Theme::dark().accent,
                selected: false,
                zone: None,
                collapsed: false,
                silenced: false,
                crossfades: found.get(&clip.id).copied().unwrap_or_default(),
            }
            .ui(ui, state);
        }
        for seam in seams {
            SeamView {
                seam,
                later: &late,
                area: early_rect.union(late_rect),
                top: 0.,
                height: BOTTOM,
                viewport: VIEW,
                ink: Color32::BLACK,
                collapsed: false,
            }
            .ui(ui, state);
        }
    }

    fn harness(setup: Setup) -> Harness<'static, Setup> {
        let harness = Harness::builder()
            .with_size(VIEW.size())
            .build_ui_state(show, setup);
        harness.ctx.set_fonts(crate::ui::font::fonts());
        harness
    }

    /// Where `beats` are on screen.
    fn x(h: &Harness<'_, Setup>, beats: f64) -> f32 {
        h.state().state.grid.beats_to_x(BeatPos(beats), VIEW)
    }

    fn handle_y() -> f32 {
        TOP_OF_BODY + HANDLE / 2. + HANDLE_REACH
    }

    fn right_click(h: &mut Harness<'_, Setup>, pos: Pos2) {
        h.hover_at(pos);
        h.run_steps(3);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            });
        }
        h.run_steps(3);
    }

    fn drag(h: &mut Harness<'_, Setup>, from: Pos2, to: Pos2) {
        h.hover_at(from);
        h.run_steps(3);
        h.drag_at(from);
        h.run_steps(1);
        h.hover_at(from.lerp(to, 0.5));
        h.run_steps(1);
        h.hover_at(to);
        h.run_steps(1);
        h.drop_at(to);
        h.run_steps(2);
    }

    fn crossfade(h: &Harness<'_, Setup>) -> Fade {
        let setup = h.state();
        setup.state.find_clip(setup.late).unwrap().1.fades.crossfade
    }

    fn spans(h: &Harness<'_, Setup>) -> [(f64, f64); 2] {
        let setup = h.state();
        [setup.early, setup.late].map(|id| {
            let clip = setup.state.find_clip(id).unwrap().1;
            (clip.position.0, clip.length)
        })
    }

    /// Dragged either way from the seam, it widens evenly, snapped: a beat
    /// from the seam is half a second each side. Nothing moves.
    #[test]
    fn dragging_the_seam_makes_a_crossfade_around_it() {
        for to in [5., 3.] {
            let mut h = harness(setup());
            let before = spans(&h);
            let from = pos2(x(&h, 4.), handle_y());
            let target = pos2(x(&h, to), handle_y());
            drag(&mut h, from, target);
            assert_eq!(crossfade(&h).length_s, 1., "dragged to {to}");
            assert_eq!(spans(&h), before, "the clips stay");
        }
    }

    #[test]
    fn dragging_back_to_the_seam_removes_the_crossfade() {
        let mut h = harness(with_crossfade(1.));
        // Half a second each side: its handle a beat after the seam.
        let from = pos2(x(&h, 5.), handle_y());
        let to = pos2(x(&h, 4.), handle_y());
        drag(&mut h, from, to);
        assert_eq!(crossfade(&h).length_s, 0.);
    }

    /// With nothing trimmed off either side, there's nothing to cross with.
    #[test]
    fn a_seam_with_no_trimmed_audio_stays_without_a_crossfade() {
        let mut setup = setup();
        let bpm = setup.state.bpm();
        // Two whole files.
        let (track, _) = setup.state.find_clip(setup.late).unwrap();
        setup.state.delete_clips(&[setup.early, setup.late]);
        let (early, late) = (setup.state.new_clip_id(), setup.state.new_clip_id());
        let clip = |id, at| AudioClip::new(id, AudioInfo::silent(2.), BeatPos(at), bpm);
        setup
            .state
            .add_clips(&track, vec![clip(early, 0.), clip(late, 4.)]);
        (setup.early, setup.late) = (early, late);
        let mut h = harness(setup);
        let from = pos2(x(&h, 4.), handle_y());
        let to = pos2(x(&h, 5.), handle_y());
        drag(&mut h, from, to);
        assert_eq!(crossfade(&h).length_s, 0.);
    }

    #[test]
    fn dragging_a_crossfade_bends_the_later_clips_curve() {
        let mut h = harness(with_crossfade(1.));
        // Half way to the seam on the rising curve.
        let curve = crossfade(&h).curve;
        let on = pos2(
            x(&h, 3.5),
            BOTTOM - curve.gain(0.25) * (BOTTOM - TOP_OF_BODY),
        );
        drag(&mut h, on, on - vec2(0., 20.));
        let fade = crossfade(&h);
        assert!(fade.curve.curvature > 0.);
        assert_eq!(fade.length_s, 1., "the length stays");
    }

    #[test]
    fn a_seams_menu_picks_its_shape_and_resets_it() {
        let mut h = harness(with_crossfade(1.));
        let handle = pos2(x(&h, 5.), handle_y());
        right_click(&mut h, handle);
        h.get_by_label("Linear").click();
        h.run_steps(3);
        assert_eq!(crossfade(&h).curve.shape, FadeShape::Linear);
        right_click(&mut h, handle);
        h.get_by_label("Reset").click();
        h.run_steps(3);
        assert_eq!(crossfade(&h), ClipFades::default().crossfade);
    }

    #[test]
    fn double_clicking_a_seam_makes_a_short_crossfade() {
        let mut h = Harness::builder()
            .with_size(VIEW.size())
            .with_step_dt(0.02)
            .build_ui_state(show, setup());
        h.ctx.set_fonts(crate::ui::font::fonts());
        let handle = pos2(x(&h, 4.), handle_y());
        h.hover_at(handle);
        h.run_steps(3);
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton {
                    pos: handle,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
                h.run_steps(1);
            }
        }
        h.run_steps(3);
        assert_eq!(crossfade(&h).length_s, DEFAULT_CROSSFADE_S);
    }

    /// At a seam, one handle: the clips' own fades have none there, unless
    /// there's a fade to get hold of.
    #[test]
    fn a_seam_has_one_handle() {
        let mut h = harness(setup());
        // Where the earlier clip's fade out would have its handle.
        let from = pos2(x(&h, 4.) - TRIM_HANDLE_WIDTH - HANDLE, handle_y());
        let to = pos2(x(&h, 3.), handle_y());
        drag(&mut h, from, to);
        let setup = h.state();
        let early = setup.state.find_clip(setup.early).unwrap().1;
        assert_eq!(early.fades.fade_out.length_s, 0.);
    }

    #[test]
    fn a_fades_menu_picks_its_shape_and_resets_it() {
        let mut setup = setup();
        let bpm = setup.state.bpm();
        let mut clip = setup.state.find_clip(setup.early).unwrap().1;
        clip.set_fade_in(1., Crossfades::default(), bpm);
        let early = clip.id;
        setup.state.commit_resize_clip(clip);
        let mut h = harness(setup);
        let fade_in =
            |h: &Harness<'_, Setup>| h.state().state.find_clip(early).unwrap().1.fades.fade_in;
        // A second is 2 beats, its handle there, at the top of the body.
        let handle = pos2(x(&h, 2.), handle_y());
        right_click(&mut h, handle);
        h.get_by_label("Exponential").click();
        h.run_steps(3);
        assert_eq!(fade_in(&h).curve.shape, FadeShape::Exponential);
        assert_eq!(fade_in(&h).length_s, 1.);
        right_click(&mut h, handle);
        h.get_by_label("Reset").click();
        h.run_steps(3);
        assert_eq!(fade_in(&h), Fade::default());
    }

    #[test]
    fn lengths_read_in_milliseconds_then_seconds() {
        assert_eq!(format_length(0.25), "250 ms");
        assert_eq!(format_length(1.5), "1.50 s");
    }
}
