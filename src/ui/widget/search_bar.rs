use std::time::{Duration, Instant};

use egui::{
    Align, Color32, CursorIcon, FontFamily, FontId, Frame, Key, Layout, Margin, RichText, Sense,
    Shape, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, vec2,
};
use egui_phosphor::regular::{MAGNIFYING_GLASS, X};

use crate::ui::{
    font::PHOSPHOR_REGULAR, theme::PRIMARY_COLOR, widget::square_button::SquareButton,
};

const HEIGHT: f32 = 22.;
const PADDING_X: f32 = 6.;
const GAP: f32 = 4.;
const ICON_SIZE: f32 = 12.;
const FONT_SIZE: f32 = 11.;
/// How long typing must pause before the query is reported.
const DEBOUNCE: Duration = Duration::from_millis(300);

const BG_COLOR: Color32 = Color32::from_gray(180);
const BORDER_COLOR: Color32 = Color32::from_gray(100);
const TEXT_COLOR: Color32 = Color32::from_gray(30);
const HINT_COLOR: Color32 = Color32::from_gray(100);

/// Single-line search field with a clear button, spanning the available
/// width. The query is reported once typing pauses, or right away on Enter
/// or when cleared.
pub struct SearchBar {
    hint: &'static str,
    query: String,
    /// Last query reported to the caller.
    submitted: String,
    /// When the query last changed, while a report is pending.
    edited_at: Option<Instant>,
}

impl SearchBar {
    pub fn new(hint: &'static str) -> Self {
        Self {
            hint,
            query: String::new(),
            submitted: String::new(),
            edited_at: None,
        }
    }

    /// Draws the bar; returns the query when the search should (re)run.
    pub fn ui(&mut self, ui: &mut Ui) -> Option<&str> {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
        // Painted once we know whether the field has focus.
        let background = ui.painter().add(Shape::Noop);

        let mut submit_now = false;
        let focused = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(PADDING_X, 0.)))
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    ui.label(
                        RichText::new(MAGNIFYING_GLASS)
                            .size(ICON_SIZE)
                            .color(TEXT_COLOR)
                            .family(FontFamily::Name(PHOSPHOR_REGULAR.into())),
                    );
                    // The clear button's space is always reserved, so the
                    // field doesn't resize when it appears.
                    let field = TextEdit::singleline(&mut self.query)
                        .hint_text(RichText::new(self.hint).color(HINT_COLOR))
                        .frame(Frame::NONE)
                        .margin(Margin::ZERO)
                        .font(FontId::new(FONT_SIZE, FontFamily::Proportional))
                        .text_color(TEXT_COLOR)
                        .desired_width(ui.available_width() - GAP - ICON_SIZE)
                        .show(ui)
                        .response;
                    if field.changed() {
                        self.edited_at = Some(Instant::now());
                    }
                    if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        submit_now = true;
                    }

                    if !self.query.is_empty() {
                        let clear = ui
                            .add(
                                SquareButton::ghost(X)
                                    .square(ICON_SIZE)
                                    .border_radius(ICON_SIZE / 2.)
                                    .color(TEXT_COLOR)
                                    .font(FontId::new(
                                        FONT_SIZE,
                                        FontFamily::Name(PHOSPHOR_REGULAR.into()),
                                    )),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand);
                        if clear.clicked() {
                            self.query.clear();
                            submit_now = true;
                            field.request_focus();
                        }
                    }
                    field.has_focus()
                },
            )
            .inner;

        let border = if focused { PRIMARY_COLOR } else { BORDER_COLOR };
        ui.painter()
            .set(background, Shape::rect_filled(rect, 2.0, BG_COLOR));
        ui.painter()
            .rect_stroke(rect, 2.0, Stroke::new(1.0, border), StrokeKind::Inside);

        let due = self.edited_at.is_some_and(|t| t.elapsed() >= DEBOUNCE);
        if submit_now || due {
            self.edited_at = None;
            if self.query != self.submitted {
                self.submitted.clone_from(&self.query);
                return Some(&self.submitted);
            }
        } else if let Some(edited_at) = self.edited_at {
            // Wake up when the pause is over, even if nothing else repaints.
            ui.ctx()
                .request_repaint_after(DEBOUNCE.saturating_sub(edited_at.elapsed()));
        }
        None
    }
}
