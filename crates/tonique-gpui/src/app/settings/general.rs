//! The General tab: the metronome's level, applied as it's dragged.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Bounds, Context, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, canvas, div,
    prelude::*, px,
};

use crate::{
    app::{
        Tonique,
        settings::{
            CONTROL_HEIGHT, CONTROL_WIDTH, button_row, section_header, setting_row, setting_rows,
        },
    },
    config::settings::Settings,
    geometry::Rect,
    theme::Theme,
    widgets::{Scale, SliderDrag, dialog_button, paint_param_slider},
};

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

impl Tonique {
    /// Applies and saves the metronome's level.
    fn set_metronome_level(&mut self, level: f32) {
        self.settings.draft.metronome_level = level;
        let mut settings = self.state.settings().clone();
        settings.metronome_level = level;
        self.state.apply_settings(settings);
    }

    fn level_down(&mut self, event: &MouseDownEvent) {
        let level = self.state.settings().metronome_level;
        if event.click_count >= 2 {
            self.settings.general.level_drag = None;
            self.set_metronome_level(level_scale().default_value());
            return;
        }
        let pointer_x = f32::from(event.position.x);
        let drag = SliderDrag::new(level_scale(), level, pointer_x, event.modifiers.shift);
        self.settings.general.level_drag = Some(drag);
    }

    pub(super) fn general_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        let current = self.state.settings().metronome_level;
        let width = f32::from(self.settings.general.level_bounds.get().size.width);
        let Some(drag) = self.settings.general.level_drag.as_mut() else {
            return false;
        };
        let pointer_x = f32::from(event.position.x);
        if let Some(level) = drag.follow(pointer_x, event.modifiers.shift, width, current) {
            self.set_metronome_level(level);
        }
        true
    }

    pub(super) fn general_drag_end(&mut self) {
        self.settings.general.level_drag = None;
    }

    pub(super) fn general_tab(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let level = self.state.settings().metronome_level;
        let slot = self.settings.general.level_bounds.clone();
        let canvas_theme = theme.clone();
        let accent = theme.accent;
        let slider = div()
            .w(px(CONTROL_WIDTH))
            .h(px(CONTROL_HEIGHT))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    this.level_down(event);
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
                            false,
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
                    this.set_metronome_level(Settings::default().metronome_level);
                    context.notify();
                }),
            );
        div()
            .child(section_header("Interface", theme))
            .child(setting_rows([setting_row("Metronome level", slider, theme)]).mt(px(6.)))
            .child(button_row(theme).justify_end().child(restore))
    }
}
