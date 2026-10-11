//! A gain or a pan as a horizontal bar, as in track headers: filled in a
//! colour from the left (gain) or from the centre (pan) to show the level,
//! the value printed on top. Dragging moves it from where it was grabbed
//! (Shift for fine steps), double-clicking resets it.

use crate::{
    ui::theme::{ThemeExt, font_size},
    utils::{format_pan, snap_to_centre},
};
use egui::{Align2, Color32, CursorIcon, FontId, Rect, Response, Sense, Ui, Vec2, pos2, vec2};
use std::ops::RangeInclusive;

/// Shift-dragging moves this much slower.
const FINE_DRAG: f32 = 0.1;
const PAN_RANGE: RangeInclusive<f32> = -1.0..=1.0;

#[derive(Clone, Debug, PartialEq)]
enum Scale {
    /// Linear gain, shown in dB over this range; its bottom is silence.
    Gain(RangeInclusive<f32>),
    /// -1 (left) to 1 (right), snapping to the centre.
    Pan,
}

/// What the user did, in the parameter's own values (linear gain, or pan).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderEdit {
    None,
    /// Dragged to this value: set it live.
    Moved(f32),
    /// The drag ended at `value`, from `start`: one undo step.
    Released {
        start: f32,
        value: f32,
    },
    /// Double-clicked: back to the default (unity gain, centre), from
    /// `from`.
    Reset {
        from: f32,
        to: f32,
    },
}

/// A gain or pan slider. The parameter is the source of truth: pass its
/// value every frame, and apply the edits returned.
pub struct ParamSlider {
    scale: Scale,
    value: f32,
    color: Color32,
    size: Vec2,
    suffix: &'static str,
}

impl ParamSlider {
    /// For a linear `gain`, shown in dB over `db` (whose bottom is silence).
    pub fn gain(gain: f32, db: RangeInclusive<f32>) -> Self {
        Self::new(Scale::Gain(db), gain)
    }

    /// For a `pan` from -1 (left) to 1 (right).
    pub fn pan(pan: f32) -> Self {
        Self::new(Scale::Pan, pan)
    }

    fn new(scale: Scale, value: f32) -> Self {
        Self {
            scale,
            value,
            color: Color32::GRAY,
            size: vec2(31., 20.),
            suffix: "",
        }
    }

    /// The fill's colour.
    pub fn color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    /// Printed after a gain's value, like ` dB`.
    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn show(self, ui: &mut Ui) -> (Response, SliderEdit) {
        let (rect, mut response) = ui.allocate_exact_size(self.size, Sense::click_and_drag());
        response = response.on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);
        let range = self.range();
        let mut shown = self.slider_value(self.value);
        let mut edit = SliderEdit::None;

        let drag_id = response.id.with("drag");
        let mut drag: Option<Drag> = ui.data(|d| d.get_temp(drag_id));
        match anchored_drag(ui, &response, &mut drag, shown, self.value, &range) {
            DragStep::Moved(value) => {
                shown = value;
                edit = SliderEdit::Moved(self.param_value(value));
                response.mark_changed();
            }
            DragStep::Released { start } => {
                edit = SliderEdit::Released {
                    start,
                    value: self.value,
                };
            }
            DragStep::None => {}
        }
        ui.data_mut(|d| match drag {
            Some(drag) => {
                d.insert_temp(drag_id, drag);
            }
            None => d.remove::<Drag>(drag_id),
        });
        let default = self.default();
        if response.double_clicked() && self.value != default {
            shown = self.slider_value(default);
            edit = SliderEdit::Reset {
                from: self.value,
                to: default,
            };
            response.mark_changed();
        }

        let fill = match self.scale {
            Scale::Gain(_) => {
                let t = (shown - range.start()) / (range.end() - range.start());
                Rect::from_min_max(
                    rect.min,
                    pos2(rect.lerp_inside(vec2(t.clamp(0., 1.), 0.)).x, rect.max.y),
                )
            }
            Scale::Pan => {
                let centre = rect.center().x;
                let x = rect.lerp_inside(vec2((shown + 1.) / 2., 0.)).x;
                Rect::from_x_y_ranges(centre.min(x)..=centre.max(x), rect.y_range())
            }
        };
        let text = match self.scale {
            Scale::Gain(_) => format!("{}{}", format_db(self.param_value(shown)), self.suffix),
            Scale::Pan => format_pan(shown),
        };
        let theme = ui.app_theme();
        let painter = ui.painter();
        painter.rect_filled(rect, 1.0, theme.bg_deep);
        painter.rect_filled(fill, 1.0, self.color);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(font_size::SMALL),
            theme.text,
        );
        (response, edit)
    }

    fn range(&self) -> RangeInclusive<f32> {
        match &self.scale {
            Scale::Gain(db) => db.clone(),
            Scale::Pan => PAN_RANGE,
        }
    }

    fn default(&self) -> f32 {
        match self.scale {
            Scale::Gain(_) => 1.,
            Scale::Pan => 0.,
        }
    }

    /// The slider's position (dB, or pan) for a parameter value.
    fn slider_value(&self, value: f32) -> f32 {
        match &self.scale {
            Scale::Gain(db) if value > 0. => (20. * value.log10()).clamp(*db.start(), *db.end()),
            Scale::Gain(db) => *db.start(),
            Scale::Pan => value.clamp(-1., 1.),
        }
    }

    /// The parameter value for a slider position.
    fn param_value(&self, value: f32) -> f32 {
        match &self.scale {
            Scale::Gain(db) if value <= *db.start() => 0.,
            Scale::Gain(_) => 10f32.powf(value / 20.),
            Scale::Pan => snap_to_centre(value),
        }
    }
}

/// A linear gain in dB: `-3.2`, or `-inf` for silence.
pub fn format_db(gain: f32) -> String {
    if gain > 0. {
        format!("{:.1}", 20. * gain.log10())
    } else {
        "-inf".into()
    }
}

/// A slider drag: the value follows the pointer's distance from where it
/// was anchored, so going past an end and back lands where it left.
#[derive(Debug, Clone, Copy)]
struct Drag {
    /// The parameter's value when the drag started, for undo.
    start: f32,
    anchor_x: f32,
    /// Slider value at `anchor_x`.
    anchor_value: f32,
    /// Shift held: slower, for fine adjustment.
    fine: bool,
}

enum DragStep {
    None,
    /// Dragged to this slider value.
    Moved(f32),
    /// Released: commit the change from the parameter's `start` value.
    Released {
        start: f32,
    },
}

/// Drive `drag` from `response`. `value` is the slider's current value in
/// `range`, `param` the parameter's (they differ for gain: dB vs linear).
fn anchored_drag(
    ui: &Ui,
    response: &Response,
    drag: &mut Option<Drag>,
    value: f32,
    param: f32,
    range: &RangeInclusive<f32>,
) -> DragStep {
    let fine = ui.input(|i| i.modifiers.shift);
    let pointer_x = response.interact_pointer_pos().map(|p| p.x);
    let value = value.clamp(*range.start(), *range.end());

    // From where the button went down: egui starts drags a few points
    // later, which shouldn't be lost.
    let press_x = ui.input(|i| i.pointer.press_origin()).map(|p| p.x);
    if response.drag_started()
        && let Some(x) = press_x.or(pointer_x)
    {
        *drag = Some(Drag {
            start: param,
            anchor_x: x,
            anchor_value: value,
            fine,
        });
    }
    if response.drag_stopped()
        && let Some(drag) = drag.take()
    {
        return DragStep::Released { start: drag.start };
    }
    if response.dragged()
        && let (Some(drag), Some(x)) = (drag.as_mut(), pointer_x)
    {
        // Toggling Shift re-anchors, so the value doesn't jump.
        if drag.fine != fine {
            *drag = Drag {
                anchor_x: x,
                anchor_value: value,
                fine,
                ..*drag
            };
        }
        let dragged = dragged_value(drag, x, response.rect.width(), range);
        if dragged != value {
            return DragStep::Moved(dragged);
        }
    }
    DragStep::None
}

/// Slider value with the pointer at `x`, on a slider `width` wide.
fn dragged_value(drag: &Drag, x: f32, width: f32, range: &RangeInclusive<f32>) -> f32 {
    let speed = if drag.fine { FINE_DRAG } else { 1. };
    let per_px = (range.end() - range.start()) / width * speed;
    (drag.anchor_value + (x - drag.anchor_x) * per_px).clamp(*range.start(), *range.end())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 45 dB over 90 px: 0.5 dB per pixel.
    const WIDTH: f32 = 90.;
    const GAIN: RangeInclusive<f32> = -40.0..=5.0;

    fn drag(fine: bool) -> Drag {
        Drag {
            start: 1.,
            anchor_x: 100.,
            anchor_value: 0.,
            fine,
        }
    }

    #[test]
    fn value_follows_the_pointer_past_the_ends() {
        let drag = drag(false);
        assert_eq!(dragged_value(&drag, 104., WIDTH, &GAIN), 2.);
        // Past the top it stays at the top...
        assert_eq!(dragged_value(&drag, 150., WIDTH, &GAIN), 5.);
        // ...and coming back gives the same value as on the way out.
        assert_eq!(dragged_value(&drag, 104., WIDTH, &GAIN), 2.);
        assert_eq!(dragged_value(&drag, -500., WIDTH, &GAIN), -40.);
        assert_eq!(dragged_value(&drag, 100., WIDTH, &GAIN), 0.);
    }

    #[test]
    fn shift_drags_ten_times_slower() {
        assert_eq!(dragged_value(&drag(true), 120., WIDTH, &GAIN), 1.);
        // Pan drags over -1..1 like any slider.
        assert_eq!(dragged_value(&drag(false), 145., WIDTH, &PAN_RANGE), 1.);
    }

    #[test]
    fn gains_map_to_decibels_down_to_silence() {
        let slider = ParamSlider::gain(1., GAIN);
        assert_eq!(slider.slider_value(1.), 0.);
        assert_eq!(slider.slider_value(0.), -40., "silence at the bottom");
        assert_eq!(slider.param_value(-40.), 0.);
        assert!((slider.param_value(-6.0206) - 0.5).abs() < 1e-4);
        assert_eq!(format_db(1.), "0.0");
        assert_eq!(format_db(0.5), "-6.0");
        assert_eq!(format_db(0.), "-inf");
        // Pans snap to the centre.
        assert_eq!(ParamSlider::pan(0.).param_value(0.01), 0.);
    }

    /// Frames of a slider at (0, 0) showing `value`, one per batch of
    /// events: what it returned each time.
    fn run(
        slider: impl Fn(f32) -> ParamSlider,
        value: &mut f32,
        frames: Vec<Vec<egui::Event>>,
    ) -> Vec<SliderEdit> {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut edits = Vec::new();
        for (i, events) in frames.into_iter().enumerate() {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400., 100.))),
                // A tenth of a second apart: clicks close enough to double.
                time: Some(i as f64 / 10.),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let edit = slider(*value).size(vec2(100., 20.)).show(ui).1;
                match edit {
                    SliderEdit::Moved(v) | SliderEdit::Reset { to: v, .. } => *value = v,
                    _ => {}
                }
                edits.push(edit);
            });
            output.textures_delta.clear();
        }
        edits
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn double_clicking_resets_to_unity_and_centre() {
        let at = egui::pos2(50., 10.);
        let click = || {
            vec![
                egui::Event::PointerMoved(at),
                button(at, true),
                button(at, false),
            ]
        };
        let mut gain = 0.25;
        let edits = run(
            |g| ParamSlider::gain(g, GAIN),
            &mut gain,
            vec![vec![], click(), click()],
        );
        assert_eq!(
            edits.last(),
            Some(&SliderEdit::Reset { from: 0.25, to: 1. })
        );
        assert_eq!(gain, 1.);

        let mut pan = -0.5;
        run(ParamSlider::pan, &mut pan, vec![vec![], click(), click()]);
        assert_eq!(pan, 0.);
    }

    #[test]
    fn dragging_moves_live_then_releases_once() {
        let (from, to) = (egui::pos2(50., 10.), egui::pos2(75., 10.));
        let mut pan = 0.;
        let edits = run(
            ParamSlider::pan,
            &mut pan,
            vec![
                vec![],
                vec![egui::Event::PointerMoved(from), button(from, true)],
                vec![egui::Event::PointerMoved(to)],
                vec![button(to, false)],
            ],
        );
        // A quarter of the slider: half of the way right.
        assert!((pan - 0.5).abs() < 0.02, "{pan}");
        assert!(edits.iter().any(|e| matches!(e, SliderEdit::Moved(_))));
        assert!(matches!(
            edits.last(),
            Some(SliderEdit::Released { start, .. }) if *start == 0.
        ));
    }
}
