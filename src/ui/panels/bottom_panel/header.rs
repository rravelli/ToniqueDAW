//! The panel's header: the track's name, the clip's, and the tabs.

use crate::ui::font::semibold;
use crate::ui::theme::font_size;
use crate::{
    core::track::TrackRow,
    ui::{
        theme::{ThemeExt, with_alpha},
        workspace::BottomView,
    },
    utils::display_name,
};
use egui::{
    Align, Color32, CursorIcon, FontId, Frame, Galley, Label, Layout, Margin, Mesh, Painter, Pos2,
    Rangef, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};
use std::sync::Arc;
use tonique_engine::edit::ClipId;

use super::{BottomPanel, HEADER_ROW, TAB_PADDING, TAB_RADIUS};

impl BottomPanel {
    /// Track name, and the clip's when showing one. With `tabs` (the view
    /// shown, and a clip of the track), Clip and Effects tabs on the right
    /// switch between them: returns the one picked.
    pub(super) fn header(
        &mut self,
        ui: &mut Ui,
        track: &TrackRow,
        clip: Option<(&str, &std::path::Path)>,
        tabs: Option<(BottomView, ClipId)>,
    ) -> Option<BottomView> {
        let theme = ui.app_theme();
        let ink = theme.text_on(track.color);
        let mut picked = None;
        // Drawn once the header is: (where, name, selected, hovered).
        let mut drawn: Vec<(Rect, Arc<Galley>, bool, bool)> = Vec::new();
        let header = Frame::new()
            .fill(track.color)
            .stroke(Stroke::new(1.0, theme.border))
            .inner_margin(Margin {
                bottom: 1,
                top: 1,
                left: 5,
                right: 5,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let row = vec2(ui.available_width(), HEADER_ROW);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    // Room for the selected tab's feet, clear of its neighbour.
                    ui.spacing_mut().item_spacing.x = TAB_RADIUS + 2.;
                    if let Some((shown, clip)) = tabs {
                        // Right to left: Effects, then Clip.
                        for (view, name) in [
                            (BottomView::Effects, "Effects"),
                            (BottomView::Clip(clip), "Clip"),
                        ] {
                            let selected = view == shown;
                            let galley = ui.painter().layout_no_wrap(
                                name.into(),
                                FontId::proportional(font_size::SMALL),
                                Color32::PLACEHOLDER,
                            );
                            let size = vec2(galley.size().x + 2. * TAB_PADDING, HEADER_ROW);
                            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                            if !selected {
                                let response = response.on_hover_cursor(CursorIcon::PointingHand);
                                if response.clicked() {
                                    picked = Some(view);
                                }
                                drawn.push((rect, galley, false, response.hovered()));
                            } else {
                                drawn.push((rect, galley, true, false));
                            }
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let mut title = display_name(&track.name, track.first_track_index);
                        if let Some((name, _)) = clip {
                            title = format!("{title}  ›  {name}");
                        }
                        let label = ui.add(
                            Label::new(
                                RichText::new(title)
                                    .font(semibold(font_size::SMALL))
                                    .color(ink),
                            )
                            .truncate()
                            .selectable(false),
                        );
                        if let Some((_, path)) = clip {
                            label.on_hover_text(path.display().to_string());
                        }
                    });
                });
            })
            .response
            .rect;
        // The selected tab in the panel's colour, open onto it: the view
        // under the header is that tab's. The others in the header's ink,
        // tinted with it under the pointer.
        let painter = ui.painter();
        // The selected one first: nothing of it over the others.
        drawn.sort_by_key(|(.., selected, _)| !selected);
        for (rect, galley, selected, hovered) in drawn {
            let text = if selected {
                paint_tab(painter, rect.x_range(), header, theme.bg_panel);
                theme.text
            } else {
                if hovered {
                    painter.rect_filled(rect, TAB_RADIUS, with_alpha(ink, 50));
                }
                ink
            };
            let pos = rect.center() - galley.size() / 2.;
            painter.galley(pos, galley, text);
        }
        picked
    }
}

/// A selected tab across `x`, from near the top of `header` down to its
/// bottom edge, in `fill`: rounded on top, its foot curving out on each
/// side, so it reads as part of the panel below. One shape, kept inside
/// the header, its curved edges smoothed by a thin line along them.
fn paint_tab(painter: &Painter, x: Rangef, header: Rect, fill: Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    const STEPS: usize = 8;
    let r = TAB_RADIUS;
    let (top, bottom) = (header.top() + 2., header.bottom());
    // A quarter circle around `centre`, from angle `from` to `to` (y down).
    let arc = |centre: Pos2, from: f32, to: f32| {
        (0..=STEPS).map(move |i| {
            let a = from + (to - from) * i as f32 / STEPS as f32;
            centre + r * vec2(a.cos(), a.sin())
        })
    };
    // Its outline left to right: the left foot, the top corners, the
    // right foot. Each x has one point on it, the bottom being flat.
    let edge: Vec<Pos2> = arc(pos2(x.min - r, bottom - r), FRAC_PI_2, 0.)
        .chain(arc(pos2(x.min + r, top + r), PI, 1.5 * PI))
        .chain(arc(pos2(x.max - r, top + r), 1.5 * PI, 2. * PI))
        .chain(arc(pos2(x.max + r, bottom - r), PI, FRAC_PI_2))
        .collect();
    let mut mesh = Mesh::default();
    for pair in edge.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let i = mesh.vertices.len() as u32;
        for p in [a, b, pos2(b.x, bottom), pos2(a.x, bottom)] {
            mesh.colored_vertex(p, fill);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    let painter = painter.with_clip_rect(header);
    painter.add(mesh);
    painter.add(Shape::line(edge, Stroke::new(1., fill)));
}
