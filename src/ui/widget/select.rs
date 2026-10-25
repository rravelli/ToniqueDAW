use crate::ui::theme::font_size;
use egui::{
    Align2, CursorIcon, FontFamily, FontId, Id, Popup, Response, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, Vec2, Widget,
    text::{LayoutJob, TextWrapping},
    vec2,
};
use egui_phosphor::fill::CARET_DOWN;

use crate::ui::{font::PHOSPHOR_FILL, theme::ThemeExt, widget::list_row::ListRow};

const HEIGHT: f32 = 20.;
const PADDING: f32 = 6.;
/// The caret, at the right: its width, and the room it takes with the
/// padding at its right.
const CARET_WIDTH: f32 = 12.;
const CARET_AREA: f32 = CARET_WIDTH + PADDING;
const MAX_POPUP_HEIGHT: f32 = 240.;

/// Dropdown picking one of `options`. The response is marked changed when
/// a new option is picked.
pub struct Select<'a, T> {
    id_salt: Id,
    value: &'a mut T,
    options: Vec<(T, String)>,
    width: f32,
    height: f32,
    /// The value centred left of the caret, rather than at the left.
    centred: bool,
    placeholder: String,
}

impl<'a, T: PartialEq + Clone> Select<'a, T> {
    pub fn new(id_salt: impl std::hash::Hash + std::fmt::Debug, value: &'a mut T) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            value,
            options: Vec::new(),
            width: 160.,
            height: HEIGHT,
            centred: false,
            placeholder: "—".into(),
        }
    }

    pub fn option(mut self, value: T, label: impl ToString) -> Self {
        self.options.push((value, label.to_string()));
        self
    }

    pub fn options(mut self, options: impl IntoIterator<Item = (T, String)>) -> Self {
        self.options.extend(options);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// As wide as a field `width` wide beside it, plus the caret: the value
    /// centred over that width, as in that field.
    pub fn value_width(mut self, width: f32) -> Self {
        self.width = width + CARET_AREA;
        self.centred = true;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Shown when the value matches no option.
    pub fn placeholder(mut self, text: impl ToString) -> Self {
        self.placeholder = text.to_string();
        self
    }
}

impl<T: PartialEq + Clone> Widget for Select<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let enabled = ui.is_enabled();
        let id = ui.make_persistent_id(self.id_salt);
        let popup_id = id.with("popup");
        let open = Popup::is_id_open(ui.ctx(), popup_id);
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(self.width, self.height), Sense::click());

        let theme = ui.app_theme();
        let fill = if !enabled {
            theme.bg_raised
        } else if open || response.hovered() {
            theme.bg_control_hover
        } else {
            theme.bg_control
        };
        let stroke = if open || response.has_focus() {
            Stroke::new(1., theme.accent)
        } else {
            Stroke::new(1., theme.control_border)
        };
        let text_color = if enabled {
            theme.text
        } else {
            theme.text_disabled
        };
        let painter = ui.painter();
        painter.rect(rect, 2., fill, stroke, StrokeKind::Inside);

        let selected = self
            .options
            .iter()
            .find(|(v, _)| v == self.value)
            .map_or(self.placeholder.clone(), |(_, label)| label.clone());
        let mut job = LayoutJob::simple_singleline(
            selected,
            FontId::proportional(font_size::BODY),
            text_color,
        );
        job.wrap = TextWrapping::truncate_at_width(rect.width() - 2. * PADDING - CARET_WIDTH);
        let galley = painter.layout_job(job);
        let (align, area) = if self.centred {
            (
                Align2::CENTER_CENTER,
                rect.with_max_x(rect.right() - CARET_AREA),
            )
        } else {
            (Align2::LEFT_CENTER, rect.shrink2(Vec2::new(PADDING, 0.)))
        };
        painter.galley(
            align.align_size_within_rect(galley.size(), area).min,
            galley,
            text_color,
        );
        painter.text(
            rect.right_center() - vec2(PADDING, 0.),
            Align2::RIGHT_CENTER,
            CARET_DOWN,
            FontId::new(9., FontFamily::Name(PHOSPHOR_FILL.into())),
            text_color,
        );
        if enabled {
            response = response.on_hover_cursor(CursorIcon::PointingHand);
        }

        let mut picked = None;
        Popup::menu(&response)
            .id(popup_id)
            .width(rect.width())
            .show(|ui| {
                ui.set_min_width(rect.width());
                ScrollArea::vertical()
                    .max_height(MAX_POPUP_HEIGHT)
                    .show(ui, |ui| {
                        for (value, label) in &self.options {
                            let item = ListRow::new(label).selected(value == &*self.value);
                            if ui.add(item).clicked() {
                                picked = Some(value.clone());
                            }
                        }
                    });
            });
        if let Some(value) = picked
            && value != *self.value
        {
            *self.value = value;
            response.mark_changed();
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect};

    fn click(pos: Pos2) -> Vec<Event> {
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        vec![Event::PointerMoved(pos), button(true), button(false)]
    }

    #[test]
    fn picking_an_option_changes_the_value() {
        let ctx = Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut value = 1;
        let frame = |events: Vec<Event>, value: &mut i32| {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400., 400.))),
                events,
                ..Default::default()
            };
            let mut result = None;
            let mut output = ctx.run_ui(input, |ui| {
                let response = ui.add(
                    Select::new("select", value)
                        .option(1, "One")
                        .option(2, "Two"),
                );
                result = Some((response.rect, response.changed()));
            });
            output.textures_delta.clear();
            result.unwrap()
        };

        let (rect, _) = frame(vec![], &mut value);
        frame(click(rect.center()), &mut value);
        frame(vec![], &mut value);
        // Options are 16 px rows under the button; "Two" is the second.
        let margin = ctx.global_style().spacing.menu_margin.top as f32;
        let two = Pos2::new(rect.center().x, rect.bottom() + margin + 16. + 8.);
        let (_, changed) = frame(click(two), &mut value);
        assert_eq!(value, 2);
        assert!(changed);
    }
}
