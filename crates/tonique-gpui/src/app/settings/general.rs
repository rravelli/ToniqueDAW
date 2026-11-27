//! The General tab: the metronome's level, applied as it's dragged or
//! stepped by the arrow keys once clicked (1%, 10% with Shift).

use super::SettingsDialog;
use crate::model::HoldsProject;
use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Context, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, canvas,
    div, prelude::*, px,
};

use crate::{
    app::{
        ArrowHolder,
        settings::{
            CONTROL_HEIGHT, CONTROL_WIDTH, button_row, section_header, setting_row, setting_rows,
        },
    },
    geometry::Rect,
    theme::Theme,
    widgets::{Scale, SliderDrag, dialog_button, paint_param_slider},
};
use tonique_core::{config::settings::Settings, utils::steps::Presses};

#[derive(Default)]
pub struct GeneralForm {
    /// The level slider, as last laid out, for its drag.
    level_bounds: Rc<Cell<Bounds<Pixels>>>,
    level_drag: Option<SliderDrag>,
}

impl GeneralForm {
    pub fn is_dragging(&self) -> bool {
        self.level_drag.is_some()
    }
}

fn level_scale() -> Scale {
    Scale::Level {
        default: Settings::default().metronome_level,
    }
}

impl SettingsDialog {
    /// Applies and saves the metronome's level.
    fn set_metronome_level(&mut self, level: f32, context: &mut App) {
        self.draft.metronome_level = level;
        let mut settings = self.state(context).settings().clone();
        settings.metronome_level = level;
        self.state_mut(context).apply_settings(settings);
    }

    /// The level stepped by the arrow keys.
    pub(crate) fn step_metronome_level(&mut self, presses: Presses, context: &mut App) {
        let level = self.state(context).settings().metronome_level;
        let stepped = level_scale().stepped(level, presses);
        if stepped != level && self.general.level_drag.is_none() {
            self.set_metronome_level(stepped, context);
        }
    }

    fn level_down(&mut self, event: &MouseDownEvent, context: &mut Context<Self>) {
        let step = crate::app::arrows::stepper(context.weak_entity(), |this, presses, context| {
            this.step_metronome_level(presses, context)
        });
        crate::app::arrows::hold(ArrowHolder::MetronomeLevel, step, context);
        let level = self.state(context).settings().metronome_level;
        if event.click_count >= 2 {
            self.general.level_drag = None;
            self.set_metronome_level(level_scale().default_value(), context);
            return;
        }
        let pointer_x = f32::from(event.position.x);
        let drag = SliderDrag::new(level_scale(), level, pointer_x, event.modifiers.shift);
        self.general.level_drag = Some(drag);
    }

    pub(super) fn general_drag_move(&mut self, event: &MouseMoveEvent, context: &mut App) -> bool {
        let current = self.state(context).settings().metronome_level;
        let width = f32::from(self.general.level_bounds.get().size.width);
        let Some(drag) = self.general.level_drag.as_mut() else {
            return false;
        };
        let pointer_x = f32::from(event.position.x);
        if let Some(level) = drag.follow(pointer_x, event.modifiers.shift, width, current) {
            self.set_metronome_level(level, context);
        }
        true
    }

    pub(super) fn general_drag_end(&mut self) {
        self.general.level_drag = None;
    }

    pub(super) fn general_tab(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let level = self.state(context).settings().metronome_level;
        let slot = self.general.level_bounds.clone();
        let canvas_theme = theme.clone();
        let accent = theme.accent;
        let focused = crate::app::arrows::holds(&ArrowHolder::MetronomeLevel, context);
        let slider = div()
            .w(px(CONTROL_WIDTH))
            .h(px(CONTROL_HEIGHT))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    this.level_down(event, context);
                    context.notify();
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, context| {
                        let rect = Rect::of(bounds);
                        let scale = level_scale();
                        paint_param_slider(
                            window,
                            context,
                            rect,
                            scale,
                            level,
                            accent,
                            (false, focused),
                            &canvas_theme,
                        )
                    },
                )
                .size_full(),
            );
        let restore = dialog_button("restore-general", "Restore defaults", false, theme)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.set_metronome_level(Settings::default().metronome_level, context);
                    context.notify();
                }),
            );
        div()
            .child(section_header("Interface", theme))
            .child(setting_rows([setting_row("Metronome level", slider, theme)]).mt(px(6.)))
            .child(button_row(theme).justify_end().child(restore))
    }
}
