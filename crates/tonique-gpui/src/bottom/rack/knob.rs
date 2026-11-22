//! A knob, as the egui app's `Knob`: its name above, an arc filled to its
//! value, its value below. Dragged up or down (ten times finer with
//! Shift), on a linear or log scale; a double-click resets it. Clicked,
//! the arrow keys step it 1% of its travel (10% with Shift), each press an
//! undo step.

use std::f32::consts::{FRAC_PI_4, PI};

use gpui::{
    Context, CursorStyle, MouseButton, MouseDownEvent, Stateful, canvas, div, prelude::*, px,
};
use tonique_engine::edit::{Parameter, PluginId, PluginKind, TrackId};

use crate::{
    app::{ArrowHolder, Tonique},
    bottom::{
        paint::{Align, circle, polyline, text},
        rack::RackDrag,
    },
    core::effect::{ECHO_SYNCS, Setting},
    geometry::Rect,
    theme::{Theme, size},
    utils::steps::{Presses, step_ratio},
};

pub const KNOB_WIDTH: f32 = 46.;
/// Name, knob and value.
pub const KNOB_HEIGHT: f32 = 52.;
const RADIUS: f32 = 11.;
/// Drag distance covering the whole range. Shift drags ten times finer.
const DRAG_RANGE: f32 = 150.;
/// Where the arc starts (the minimum), clockwise from the right; it sweeps
/// 270° to the maximum.
const START_ANGLE: f32 = 3. * FRAC_PI_4;
const SWEEP: f32 = 1.5 * PI;

/// How a knob's turn maps to its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnobScale {
    pub min: f32,
    pub max: f32,
    /// Logarithmic, for frequencies and times: the range must be positive.
    pub log: bool,
}

impl KnobScale {
    pub fn ratio(self, value: f32) -> f32 {
        let ratio = if self.log {
            (value / self.min).ln() / (self.max / self.min).ln()
        } else {
            (value - self.min) / (self.max - self.min)
        };
        if ratio.is_finite() {
            ratio.clamp(0., 1.)
        } else {
            0.
        }
    }

    pub fn value(self, ratio: f32) -> f32 {
        if self.log {
            self.min * (self.max / self.min).powf(ratio)
        } else {
            self.min + (self.max - self.min) * ratio
        }
    }
}

/// What a knob sets.
#[derive(Clone)]
pub enum KnobTarget {
    /// A parameter, live while dragged, one undo step once released.
    Param(Parameter),
    /// One of an effect's settings: shown while dragged, applied once
    /// released, as it replaces the plugin.
    Setting {
        track: TrackId,
        plugin: PluginId,
        knob: SettingKnob,
    },
}

/// A setting a knob turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKnob {
    /// An echo's time, in seconds.
    Time,
    /// A synced echo's note value, as its place in [`ECHO_SYNCS`].
    Sync,
    /// A limiter's lookahead, in ms.
    Lookahead,
}

impl SettingKnob {
    /// Its value in `plugin`, if it has it.
    pub fn read(self, plugin: &PluginKind) -> Option<f32> {
        Setting::all_of(plugin)
            .into_iter()
            .find_map(|setting| match (self, setting) {
                (SettingKnob::Time, Setting::Time(time_s)) => Some(time_s),
                (SettingKnob::Sync, Setting::Sync(Some(beats))) => Some(nearest_sync(beats) as f32),
                (SettingKnob::Lookahead, Setting::Lookahead(ms)) => Some(ms),
                _ => None,
            })
    }

    /// The setting at `value`.
    pub fn setting(self, value: f32) -> Setting {
        match self {
            SettingKnob::Time => Setting::Time(value),
            SettingKnob::Sync => {
                let index = (value.round().max(0.) as usize).min(ECHO_SYNCS.len() - 1);
                Setting::Sync(Some(ECHO_SYNCS[index].0))
            }
            SettingKnob::Lookahead => Setting::Lookahead(value),
        }
    }
}

/// The place in [`ECHO_SYNCS`] of the note value nearest `beats`.
pub fn nearest_sync(beats: f32) -> usize {
    ECHO_SYNCS
        .iter()
        .enumerate()
        .min_by(|(_, first), (_, second)| {
            (first.0 - beats).abs().total_cmp(&(second.0 - beats).abs())
        })
        .map_or(0, |(index, _)| index)
}

/// A knob holding the arrow keys.
#[derive(Clone)]
pub struct KnobArrows {
    target: KnobTarget,
    scale: KnobScale,
}

impl KnobArrows {
    pub fn is(&self, other: &Self) -> bool {
        same_target(&self.target, &other.target)
    }
}

/// A knob being dragged.
pub struct KnobDrag {
    pub target: KnobTarget,
    scale: KnobScale,
    last_y: f32,
    ratio: f32,
    /// The value when the drag started.
    pub before: f32,
    pub value: f32,
}

impl KnobDrag {
    /// Follows the pointer to `pointer_y`: the new value, if it changed.
    pub fn follow(&mut self, pointer_y: f32, fine: bool) -> Option<f32> {
        let speed = if fine { 0.1 } else { 1. };
        let delta = -(pointer_y - self.last_y) / DRAG_RANGE * speed;
        self.last_y = pointer_y;
        if delta == 0. {
            return None;
        }
        self.ratio = (self.ratio + delta).clamp(0., 1.);
        let value = self.scale.value(self.ratio);
        (value != self.value).then(|| {
            self.value = value;
            value
        })
    }
}

/// What a knob shows.
pub struct Knob {
    pub name: &'static str,
    pub value: f32,
    pub scale: KnobScale,
    pub text: String,
    /// Back to it on a double-click.
    pub default: Option<f32>,
    /// Greyed out when not, as on a bypassed effect; still editable.
    pub active: bool,
}

impl Tonique {
    pub(super) fn knob_view(
        &self,
        id: impl Into<gpui::ElementId>,
        knob: Knob,
        target: KnobTarget,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let dragging = matches!(
            &self.bottom.rack.drag,
            Some(RackDrag::Knob(drag)) if same_target(&drag.target, &target)
        );
        let Knob {
            name,
            value,
            scale,
            text: shown,
            default,
            active,
        } = knob;
        let theme = theme.clone();
        let ratio = scale.ratio(value);
        let holder = ArrowHolder::Knob(KnobArrows {
            target: target.clone(),
            scale,
        });
        let focused = self.holds_arrows(&holder);
        div()
            .id(id.into())
            .flex_none()
            .w(px(KNOB_WIDTH))
            .h(px(KNOB_HEIGHT))
            .cursor(CursorStyle::ResizeUpDown)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, context| {
                        let rect = Rect::of(bounds);
                        let pointer = window.mouse_position();
                        let hovered = dragging || bounds.contains(&pointer);
                        paint_knob(
                            window,
                            context,
                            rect,
                            (name, &shown),
                            ratio,
                            active,
                            (hovered, focused),
                            &theme,
                        );
                    },
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    this.hold_arrows(holder.clone());
                    if event.click_count >= 2 {
                        this.bottom.rack.drag = None;
                        if let Some(default) = default {
                            this.set_knob(&target, value, default, true);
                        }
                    } else {
                        this.bottom.rack.drag = Some(RackDrag::Knob(KnobDrag {
                            target: target.clone(),
                            scale,
                            last_y: f32::from(event.position.y),
                            ratio,
                            before: value,
                            value,
                        }));
                    }
                    context.notify();
                }),
            )
    }

    /// The knob of `arrows` stepped by `presses`, evenly along its travel
    /// (log knobs too), as one undo step.
    pub(crate) fn step_knob(&mut self, arrows: &KnobArrows, presses: Presses) {
        if self.bottom.rack.drag.is_some() {
            return;
        }
        let value = match &arrows.target {
            KnobTarget::Param(param) => param.get(),
            KnobTarget::Setting {
                track,
                plugin,
                knob,
            } => {
                let effect = self
                    .state
                    .effects(track)
                    .into_iter()
                    .find(|effect| effect.plugin.id == *plugin);
                match effect.and_then(|effect| knob.read(&effect.plugin.kind)) {
                    Some(value) => value,
                    None => return,
                }
            }
        };
        let ratio = arrows.scale.ratio(value);
        let stepped = step_ratio(ratio, presses);
        if stepped != ratio {
            let to = arrows.scale.value(stepped);
            self.set_knob(&arrows.target, value, to, true);
        }
    }

    /// Sets what `target` sets from `before` to `value`: as an undo step
    /// when `done`, else live (a parameter) or shown (a time).
    pub(super) fn set_knob(&mut self, target: &KnobTarget, before: f32, value: f32, done: bool) {
        match target {
            KnobTarget::Param(param) => {
                param.set(value);
                if done && before != param.get() {
                    self.state.commit_params(&[(param.id, before, param.get())]);
                }
            }
            KnobTarget::Setting {
                track,
                plugin,
                knob,
            } => {
                if done {
                    self.bottom.rack.pending = None;
                    if knob.setting(value) != knob.setting(before) {
                        self.state
                            .set_effect_setting(track, *plugin, knob.setting(value));
                    }
                } else {
                    self.bottom.rack.pending = Some((*plugin, *knob, value));
                }
            }
        }
    }
}

fn same_target(first: &KnobTarget, second: &KnobTarget) -> bool {
    match (first, second) {
        (KnobTarget::Param(first), KnobTarget::Param(second)) => first.id == second.id,
        (
            KnobTarget::Setting {
                plugin: first,
                knob: first_knob,
                ..
            },
            KnobTarget::Setting {
                plugin: second,
                knob: second_knob,
                ..
            },
        ) => first == second && first_knob == second_knob,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_knob(
    window: &mut gpui::Window,
    context: &mut gpui::App,
    rect: Rect,
    (name, value): (&str, &str),
    ratio: f32,
    active: bool,
    (hovered, focused): (bool, bool),
    theme: &Theme,
) {
    let centre_x = rect.left + rect.width / 2.;
    let centre = (centre_x, rect.top + rect.height / 2.);
    text(
        window,
        context,
        name.to_string(),
        (centre_x, rect.top),
        Align::CENTER_TOP,
        size::SMALL,
        theme.text_muted,
        None,
    );
    let arc = |from: f32, to: f32| -> Vec<(f32, f32)> {
        let steps = ((to - from) * 12.).ceil().max(1.) as usize;
        (0..=steps)
            .map(|step| {
                let angle = START_ANGLE + (from + (to - from) * step as f32 / steps as f32) * SWEEP;
                (
                    centre.0 + RADIUS * angle.cos(),
                    centre.1 + RADIUS * angle.sin(),
                )
            })
            .collect()
    };
    let color = if active {
        theme.accent
    } else {
        theme.text_disabled
    };
    polyline(window, &arc(0., 1.), 3., theme.bg_deep);
    if ratio > 0. {
        polyline(window, &arc(0., ratio), 3., color);
    }
    let fill = if hovered {
        theme.bg_control_hover
    } else {
        theme.bg_control
    };
    // Holding the arrow keys: outlined in the accent.
    let border = if focused { theme.accent } else { theme.border };
    circle(window, centre, RADIUS - 3., fill, Some((1., border)));
    let angle = START_ANGLE + ratio * SWEEP;
    let (along_x, along_y) = (angle.cos(), angle.sin());
    polyline(
        window,
        &[
            (centre.0 + along_x * 2., centre.1 + along_y * 2.),
            (
                centre.0 + along_x * (RADIUS - 3.),
                centre.1 + along_y * (RADIUS - 3.),
            ),
        ],
        1.5,
        theme.text,
    );
    text(
        window,
        context,
        value.to_string(),
        (centre_x, rect.bottom()),
        Align::CENTER_BOTTOM,
        size::SMALL,
        theme.text,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_map_back_to_values() {
        let log = KnobScale {
            min: 20.,
            max: 20_000.,
            log: true,
        };
        assert!((log.ratio(632.46) - 0.5).abs() < 1e-3);
        assert!((log.value(0.5) - 632.46).abs() < 0.1);
        assert_eq!(log.ratio(1.), 0., "clamped below");
        let linear = KnobScale {
            min: 0.,
            max: 1.,
            log: false,
        };
        assert_eq!(linear.ratio(0.25), 0.25);
        assert_eq!(linear.value(0.75), 0.75);
    }

    #[test]
    fn dragging_up_turns_it_up() {
        let scale = KnobScale {
            min: 0.,
            max: 1.,
            log: false,
        };
        let mut drag = KnobDrag {
            target: KnobTarget::Setting {
                track: TrackId(1),
                plugin: PluginId(1),
                knob: SettingKnob::Time,
            },
            scale,
            last_y: 100.,
            ratio: 0.5,
            before: 0.5,
            value: 0.5,
        };
        assert_eq!(drag.follow(70., false), Some(0.7));
        let fine = drag.follow(40., true).unwrap();
        assert!((fine - 0.72).abs() < 1e-5, "{fine}");
    }
}
