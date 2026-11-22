//! Tonique's draggable `NumberInput`: dragged sideways a unit every few
//! pixels (a tenth with Shift), clicked and stepped by the arrow keys (ten
//! units with Shift), or double-clicked to type a value. Its value isn't
//! kept here: the caller passes it in and is told new ones.

use std::cell::Cell;

use gpui::{
    AnyElement, Context, CursorStyle, KeyDownEvent, MouseButton, MouseDownEvent, Window, div,
    prelude::*, px,
};

use crate::{
    app::{ArrowHolder, arrows::stepper, focus_main},
    theme::{Theme, hsla},
    utils::steps::{self, Presses},
    widgets::text_field::{FieldKey, TextField},
};

/// Pixels of sideways drag per step.
const PIXELS_PER_STEP: f32 = 3.;
/// A step with Shift held; a whole unit otherwise.
const FINE_STEP: f32 = 0.1;

/// What a field takes.
pub struct NumberSpec {
    pub min: f32,
    pub max: f32,
    pub decimals: usize,
    pub suffix: &'static str,
    /// The only values it takes, when not empty: dragging steps through
    /// them.
    pub values: Vec<f32>,
}

impl NumberSpec {
    pub fn range(min: f32, max: f32, decimals: usize) -> Self {
        Self {
            min,
            max,
            decimals,
            suffix: "",
            values: Vec::new(),
        }
    }

    pub fn values(values: impl IntoIterator<Item = f32>) -> Self {
        let values: Vec<f32> = values.into_iter().collect();
        Self {
            min: values.iter().copied().fold(f32::INFINITY, f32::min),
            max: values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            values,
            ..Self::range(0., 0., 0)
        }
    }

    pub fn suffix(self, suffix: &'static str) -> Self {
        Self { suffix, ..self }
    }

    fn nearest(&self, value: f32) -> Option<usize> {
        (0..self.values.len()).min_by(|first, second| {
            (self.values[*first] - value)
                .abs()
                .total_cmp(&(self.values[*second] - value).abs())
        })
    }

    /// `value` in range: one of `values`, or clamped.
    fn fit(&self, value: f32) -> f32 {
        match self.nearest(value) {
            Some(index) => self.values[index],
            None => value.clamp(self.min, self.max),
        }
    }

    /// `steps` steps of `step` from `value`, on the step grid.
    fn stepped(&self, value: f32, steps: f32, step: f32) -> f32 {
        match self.nearest(value) {
            Some(index) => {
                let last = self.values.len() as f32 - 1.;
                self.values[(index as f32 + steps).clamp(0., last) as usize]
            }
            None => (((value / step).round() + steps) * step).clamp(self.min, self.max),
        }
    }

    fn format(&self, value: f32) -> String {
        format!("{:.*}", self.decimals, value)
    }
}

pub struct NumberField {
    spec: NumberSpec,
    /// Typing a value, after a double-click.
    edit: Option<TextField>,
    /// While dragged: the pointer's last x coordinate, and the part of a step not
    /// taken yet.
    drag: Option<(f32, f32)>,
    /// The value as last shown, for the arrow keys.
    shown: Cell<f32>,
}

impl NumberField {
    pub fn new(spec: NumberSpec) -> Self {
        Self {
            spec,
            edit: None,
            drag: None,
            shown: Cell::new(0.),
        }
    }

    /// The value as last shown stepped by `presses` of the arrow keys
    /// (through the list, when it has one): the new one, if it changed.
    pub fn stepped(&self, presses: Presses) -> Option<f32> {
        if self.drag.is_some() || self.edit.is_some() {
            return None;
        }
        let value = self.shown.get();
        let to = self
            .spec
            .stepped(value, presses.amount(steps::NUMBER) as f32, 1.);
        (to != value).then_some(to)
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Follows a drag to `pointer_x`, the value being `value`: the new one, if it
    /// changed.
    pub fn drag_to(&mut self, pointer_x: f32, fine: bool, value: f32) -> Option<f32> {
        let (last, rest) = self.drag?;
        let rest = rest + (pointer_x - last) / PIXELS_PER_STEP;
        let steps = rest.trunc();
        self.drag = Some((pointer_x, rest - steps));
        let step = if fine && self.spec.decimals > 0 {
            FINE_STEP
        } else {
            1.
        };
        let to = self.spec.stepped(value, steps, step);
        (steps != 0. && to != value).then_some(to)
    }

    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    /// The typed value, if it's a number, in range.
    fn typed(&self) -> Option<f32> {
        let text = self.edit.as_ref()?.text.trim().to_string();
        let text = text.trim_end_matches(self.spec.suffix.trim());
        text.trim()
            .parse::<f32>()
            .ok()
            .map(|typed| self.spec.fit(typed))
    }

    /// The field, `width` by `height`, showing `value`, outlined when `held`
    /// (holding the arrow keys). `get` finds it in its view, `set` takes a
    /// new value.
    #[allow(clippy::too_many_arguments)]
    pub fn view<V: 'static>(
        &self,
        id: &'static str,
        (value, held): (f32, bool),
        (width, height): (f32, f32),
        theme: &Theme,
        window: &Window,
        context: &mut Context<V>,
        get: fn(&mut V) -> &mut NumberField,
        set: fn(&mut V, f32, &mut Context<V>),
    ) -> AnyElement {
        let field = div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(width))
            .h(px(height))
            .rounded(px(1.))
            .overflow_hidden()
            .whitespace_nowrap();
        if let Some(edit) = &self.edit {
            // Leaving it, by Enter or a click elsewhere, takes the value.
            let finish = move |this: &mut V, keep: bool, context: &mut Context<V>| {
                let field = get(this);
                let value = field.typed().filter(|_| keep);
                field.edit = None;
                if let Some(value) = value {
                    set(this, value, context);
                }
            };
            return field
                .track_focus(&edit.focus)
                .bg(hsla(theme.bg_control))
                .px(px(3.))
                .child(edit.view("", theme, window))
                .on_key_down(context.listener(
                    move |this, event: &KeyDownEvent, window, context| {
                        let Some(edit) = get(this).edit.as_mut() else {
                            return;
                        };
                        match edit.key(event) {
                            FieldKey::Submit => finish(this, true, context),
                            FieldKey::Cancel => finish(this, false, context),
                            FieldKey::Edited => {}
                            FieldKey::Ignored => return,
                        }
                        if get(this).edit.is_none() {
                            focus_main(window, context);
                        }
                        context.stop_propagation();
                        context.notify();
                    },
                ))
                .on_mouse_down_out(context.listener(move |this, _, _, context| {
                    finish(this, true, context);
                    context.notify();
                }))
                .into_any_element();
        }
        self.shown.set(value);
        let text = format!("{}{}", self.spec.format(value), self.spec.suffix);
        let dragging = self.is_dragging();
        let step = stepper(context.weak_entity(), move |this, presses, context| {
            if let Some(value) = get(this).stepped(presses) {
                set(this, value, context);
            }
        });
        let accent = hsla(theme.accent);
        field
            .when(held, |element| element.border_1().border_color(accent))
            .cursor(CursorStyle::ResizeLeftRight)
            .when(dragging, |element| element.bg(hsla(theme.hover_overlay)))
            .hover(|style| style.bg(hsla(theme.hover_overlay)))
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, window, context| {
                    context.stop_propagation();
                    crate::app::arrows::hold_in(ArrowHolder::Number(id), step.clone(), context);
                    let field = get(this);
                    if event.click_count >= 2 {
                        field.drag = None;
                        let mut edit = TextField::new(context);
                        edit.start(field.spec.format(value), window, context);
                        get(this).edit = Some(edit);
                    } else {
                        field.drag = Some((f32::from(event.position.x), 0.));
                    }
                    context.notify();
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_steps_by_units_or_tenths() {
        let mut field = NumberField::new(NumberSpec::range(10., 1000., 1));
        field.drag = Some((0., 0.));
        assert_eq!(field.drag_to(2., false, 120.), None, "under a step");
        assert_eq!(field.drag_to(4., false, 120.), Some(121.));
        let fine = field.drag_to(10., true, 121.).unwrap();
        assert!((fine - 121.2).abs() < 1e-4, "{fine}");
    }

    #[test]
    fn listed_values_step_through_the_list() {
        let mut field = NumberField::new(NumberSpec::values([2., 4., 8., 16.]));
        field.drag = Some((0., 0.));
        assert_eq!(field.drag_to(6., false, 4.), Some(16.));
        assert_eq!(field.drag_to(-30., false, 16.), Some(2.));
        assert_eq!(field.spec.fit(5.), 4.);
    }

    #[test]
    fn the_arrow_keys_step_by_units_or_tens() {
        let field = NumberField::new(NumberSpec::range(10., 1000., 1));
        field.shown.set(120.);
        assert_eq!(field.stepped(Presses::one(true, false)), Some(121.));
        assert_eq!(field.stepped(Presses::one(false, true)), Some(110.));
        field.shown.set(1000.);
        assert_eq!(field.stepped(Presses::one(true, true)), None, "at its end");
    }
}
