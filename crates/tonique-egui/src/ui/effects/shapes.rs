//! Filter shapes drawn as small icons: each is the shape's real response,
//! so it reads like the curve it gives. A picker opens a list of them by
//! name; a row of them picks with one click.

use crate::ui::{
    theme::{ThemeExt, font_size},
    widget::flat_button::FlatButton,
};
use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, Painter, Popup, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::fill::CARET_DOWN;
use tonique_engine::{dsp::Biquad, nodes::FilterMode};

use crate::ui::font::PHOSPHOR_FILL;

/// Frequencies an icon spans, in Hz, around its shape's at 1 kHz.
const ICON_HZ: (f32, f32) = (40., 25_000.);
/// Gains an icon spans, in dB: room above 0 for boosts, more below for
/// cuts.
const ICON_DB: (f32, f32) = (14., -30.);
const ICON_SR: f32 = 48_000.;
/// A shape button in a row of them.
pub const SHAPE_BUTTON: Vec2 = vec2(19., 18.);
const ROW_HEIGHT: f32 = 20.;

/// `mode`'s response drawn across `rect`.
pub fn paint_shape_icon(painter: &Painter, rect: Rect, mode: FilterMode, stroke: Stroke) {
    let (q, gain) = match mode {
        FilterMode::Peak => (1.4, 10.),
        FilterMode::LowShelf | FilterMode::HighShelf => (0.8, 10.),
        FilterMode::Notch => (2.5, 0.),
        FilterMode::BandPass => (1.2, 0.),
        FilterMode::LowPass | FilterMode::HighPass => (0.9, 0.),
    };
    let k = Biquad::new(mode, 1000., q, gain, ICON_SR);
    let (lo, hi) = ICON_HZ;
    let steps = (rect.width() * 2.).ceil().max(8.) as usize;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let hz = lo * (hi / lo).powf(t);
            let db = k.response_db(hz, ICON_SR).clamp(ICON_DB.1, ICON_DB.0);
            let y = (ICON_DB.0 - db) / (ICON_DB.0 - ICON_DB.1);
            pos2(
                rect.left() + t * rect.width(),
                rect.top() + y * rect.height(),
            )
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}

/// A row of a button per shape of `modes`, the current one lit in
/// `color`. Returns the shape clicked.
pub fn shape_row(
    ui: &mut Ui,
    modes: &[FilterMode],
    current: FilterMode,
    color: Color32,
    name: impl Fn(FilterMode) -> &'static str,
) -> Option<FilterMode> {
    let theme = ui.app_theme();
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.;
        for &mode in modes {
            let on = mode == current;
            let mut button = FlatButton::new("")
                .size(SHAPE_BUTTON)
                .border_radius(3.)
                .tooltip(name(mode));
            if on {
                button = button.fill(color);
            }
            let response = ui.add(button);
            let ink = if on { theme.bg_deep } else { theme.text_muted };
            paint_shape_icon(
                ui.painter(),
                response.rect.shrink2(vec2(3., 4.)),
                mode,
                Stroke::new(1.3, ink),
            );
            if response.clicked() && !on {
                picked = Some(mode);
            }
        }
    });
    picked
}

/// A button showing the shape and its name, opening the list of `modes`
/// to pick from. `color` tints its icon.
pub struct ShapePicker<'a> {
    id_salt: egui::Id,
    value: &'a mut FilterMode,
    modes: &'a [FilterMode],
    name: &'a dyn Fn(FilterMode) -> &'static str,
    width: f32,
    color: Option<Color32>,
}

impl<'a> ShapePicker<'a> {
    pub fn new(
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        value: &'a mut FilterMode,
        modes: &'a [FilterMode],
        name: &'a dyn Fn(FilterMode) -> &'static str,
    ) -> Self {
        Self {
            id_salt: egui::Id::new(id_salt),
            value,
            modes,
            name,
            width: 100.,
            color: None,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }
}

impl egui::Widget for ShapePicker<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = ui.app_theme();
        let popup_id = ui.make_persistent_id(self.id_salt).with("popup");
        let open = Popup::is_id_open(ui.ctx(), popup_id);
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(self.width, ROW_HEIGHT), Sense::click());
        let fill = if open || response.hovered() {
            theme.bg_control_hover
        } else {
            theme.bg_control
        };
        let stroke = if open {
            Stroke::new(1., theme.accent)
        } else {
            Stroke::new(1., theme.control_border)
        };
        let painter = ui.painter();
        painter.rect(rect, 3., fill, stroke, StrokeKind::Inside);
        let icon = Rect::from_min_size(rect.min + vec2(5., 4.), vec2(18., ROW_HEIGHT - 8.));
        let tint = self.color.unwrap_or(theme.accent);
        paint_shape_icon(painter, icon, *self.value, Stroke::new(1.5, tint));
        painter.text(
            pos2(icon.right() + 5., rect.center().y),
            Align2::LEFT_CENTER,
            (self.name)(*self.value),
            FontId::proportional(font_size::LABEL),
            theme.text,
        );
        painter.text(
            rect.right_center() - vec2(5., 0.),
            Align2::RIGHT_CENTER,
            CARET_DOWN,
            FontId::new(9., FontFamily::Name(PHOSPHOR_FILL.into())),
            theme.text_muted,
        );
        response = response.on_hover_cursor(CursorIcon::PointingHand);

        let mut picked = None;
        Popup::menu(&response).id(popup_id).show(|ui| {
            ui.spacing_mut().item_spacing.y = 1.;
            for &mode in self.modes {
                let (row, item) =
                    ui.allocate_exact_size(vec2(self.width.max(120.), ROW_HEIGHT), Sense::click());
                let on = mode == *self.value;
                if item.hovered() || on {
                    let fill = if item.hovered() {
                        theme.bg_control_hover
                    } else {
                        theme.bg_control
                    };
                    ui.painter().rect_filled(row, 3., fill);
                }
                let icon = Rect::from_min_size(row.min + vec2(5., 4.), vec2(18., ROW_HEIGHT - 8.));
                let ink = if on { tint } else { theme.text_muted };
                paint_shape_icon(ui.painter(), icon, mode, Stroke::new(1.5, ink));
                ui.painter().text(
                    pos2(icon.right() + 6., row.center().y),
                    Align2::LEFT_CENTER,
                    (self.name)(mode),
                    FontId::proportional(font_size::LABEL),
                    theme.text,
                );
                if item.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    picked = Some(mode);
                }
            }
        });
        if let Some(mode) = picked
            && mode != *self.value
        {
            *self.value = mode;
            response.mark_changed();
        }
        response
    }
}
