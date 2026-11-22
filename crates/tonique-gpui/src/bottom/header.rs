//! The panel's header: the track's name (and the clip's, when showing
//! one) on the track's colour, and the Clip and Effects tabs on its right.

use std::f32::consts::{FRAC_PI_2, PI};

use gpui::{Context, MouseButton, canvas, div, prelude::*, px};
use tonique_engine::edit::ClipId;

use crate::{
    app::Tonique,
    bottom::{BottomView, paint::polygon},
    core::{clip::AudioClip, track::TrackRow},
    theme::{Color, hsla, size, with_alpha},
    utils::display_name,
};

/// Height of the header's row: the track's name, and tabs.
const HEADER_ROW: f32 = 14.;
/// Space each side of a tab's name.
const TAB_PADDING: f32 = 6.;
/// Rounding of a tab's top corners, and of the curves at its foot.
const TAB_RADIUS: f32 = 4.;
/// From the header's top to a selected tab's.
const TAB_TOP: f32 = 2.;

impl Tonique {
    /// Track name, and the clip's when showing one. With `tabs` (the view
    /// shown, and a clip of the track), Clip and Effects tabs on the right
    /// switch between them.
    pub(super) fn bottom_header(
        &self,
        track: &TrackRow,
        clip: Option<&AudioClip>,
        tabs: Option<(BottomView, ClipId)>,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let color = track.color;
        let ink = theme.text_on(color);
        let mut title = display_name(&track.name, track.first_track_index);
        if let Some(clip) = clip {
            title = format!("{title}  ›  {}", clip.audio.name);
        }
        let mut row = div()
            .flex()
            .flex_none()
            .items_end()
            .h(px(HEADER_ROW + 4.))
            .px(px(5.))
            .bg(hsla(color))
            .border_1()
            .border_color(hsla(theme.border))
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(size::SMALL))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(hsla(ink))
                    .child(title),
            );
        if let Some((shown, clip)) = tabs {
            let panel = theme.bg_panel;
            let text = theme.text;
            for (index, (view, name)) in [
                (BottomView::Clip(clip), "Clip"),
                (BottomView::Effects, "Effects"),
            ]
            .into_iter()
            .enumerate()
            {
                let selected = view == shown;
                let tab = div()
                    .id(("bottom-tab", index))
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .ml(px(TAB_RADIUS + 2.))
                    .px(px(TAB_PADDING))
                    .text_size(px(size::SMALL));
                let tab = if selected {
                    // In the panel's colour, open onto it.
                    tab.h(px(HEADER_ROW + 2. - TAB_TOP + 1.))
                        .mb(px(-1.))
                        .text_color(hsla(text))
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    let bounds = crate::geometry::Rect::of(bounds);
                                    paint_tab(
                                        window,
                                        (bounds.left, bounds.right()),
                                        (bounds.top, bounds.bottom()),
                                        panel,
                                    )
                                },
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(div().relative().child(name))
                } else {
                    tab.h(px(HEADER_ROW))
                        .mb(px(1.))
                        .rounded(px(TAB_RADIUS))
                        .text_color(hsla(ink))
                        .cursor_pointer()
                        .hover(move |style| style.bg(hsla(with_alpha(ink, 50))))
                        .child(name)
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(move |this, _, _, context| {
                                context.stop_propagation();
                                this.show_bottom_view(view);
                                context.notify();
                            }),
                        )
                };
                row = row.child(tab);
            }
        }
        row
    }
}

/// A selected tab across `x`, from `y`'s top to its bottom, in `fill`:
/// rounded on top, its foot curving out on each side, so it reads as part
/// of the panel below.
fn paint_tab(window: &mut gpui::Window, x: (f32, f32), y: (f32, f32), fill: Color) {
    const STEPS: usize = 8;
    let radius = TAB_RADIUS;
    let (top, bottom) = y;
    let arc = |centre: (f32, f32), from: f32, to: f32| {
        (0..=STEPS).map(move |step| {
            let angle = from + (to - from) * step as f32 / STEPS as f32;
            (
                centre.0 + radius * angle.cos(),
                centre.1 + radius * angle.sin(),
            )
        })
    };
    let outline: Vec<(f32, f32)> = arc((x.0 - radius, bottom - radius), FRAC_PI_2, 0.)
        .chain(arc((x.0 + radius, top + radius), PI, 1.5 * PI))
        .chain(arc((x.1 - radius, top + radius), 1.5 * PI, 2. * PI))
        .chain(arc((x.1 + radius, bottom - radius), PI, FRAC_PI_2))
        .collect();
    polygon(window, &outline, fill);
}
