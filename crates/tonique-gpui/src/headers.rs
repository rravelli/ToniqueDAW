//! A header per track, beside its lane, laid out as Tonique's `TrackHeader`
//! (`HeaderLayout`): the colour bar on the left, the meter down the right
//! edge, between them a first line (caret, name, mute and solo), and the
//! gain slider below it when the row is tall enough. Without groups, pan,
//! input, monitor or arm.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    TextRun, Window, canvas, div, point, prelude::*, px,
};
use tonique_engine::edit::TrackId;

use crate::{
    app::Tonique,
    core::track::{TrackRow, TrackSoloState},
    icons,
    meter::paint_meter,
    theme::{Color, FONT_UI, Theme, h, icon_fill, size, with_alpha},
    timeline::{ROW_GAP, RULER_H, rows},
    utils::display_name,
    wave::R,
};

// As `ui/arrangement/track_header.rs`.
const PADDING: f32 = 4.;
const BUTTON_W: f32 = 18.;
const BUTTON_H: f32 = 16.;
const CARET_WIDTH: f32 = 14.;
const METER_WIDTH: f32 = 12.;
const GAP: f32 = 3.;
const LINE_GAP: f32 = 4.;
const SLIDER_HEIGHT: f32 = 16.;
const VOLUME_UNIT_WIDTH: f32 = 64.;
const COLOR_BAR_WIDTH: f32 = 4.;
/// Volume slider range, in dB: its bottom is silence.
const DB_MIN: f32 = -40.;
const DB_MAX: f32 = 5.;
/// Shift-dragging moves this much slower.
const FINE_DRAG: f32 = 0.1;
const HEADERS_WIDTH: f32 = 220.;

/// Where everything in a header goes, local to it.
struct HeaderLayout {
    bar: R,
    caret: R,
    name: R,
    /// Mute, solo.
    toggles: [R; 2],
    volume: Option<R>,
    meter: R,
}

impl HeaderLayout {
    fn new(width: f32, height: f32, collapsed: bool) -> Self {
        let inner = R::new(
            PADDING,
            PADDING,
            width - 2. * PADDING,
            height - 2. * PADDING,
        );
        let bar = R::new(inner.x, inner.y, COLOR_BAR_WIDTH, inner.h);
        let meter = R::new(inner.right() - METER_WIDTH, inner.y, METER_WIDTH, inner.h);
        let content = R::new(
            bar.right() + GAP,
            inner.y,
            meter.x - GAP - (bar.right() + GAP),
            inner.h,
        );
        let has_sliders = !collapsed && content.h >= BUTTON_H + LINE_GAP + SLIDER_HEIGHT;
        let line_top = if has_sliders {
            content.y
        } else {
            content.y + content.h / 2. - BUTTON_H / 2.
        };
        let caret = R::new(content.x, line_top, CARET_WIDTH, BUTTON_H);
        let solo = R::new(content.right() - BUTTON_W, line_top, BUTTON_W, BUTTON_H);
        let mute = R::new(solo.x - 1. - BUTTON_W, line_top, BUTTON_W, BUTTON_H);
        let name_left = caret.right() + 2.;
        let name = R::new(
            name_left,
            line_top,
            (mute.x - 1. - GAP - name_left).max(0.),
            BUTTON_H,
        );
        let volume = has_sliders.then(|| {
            R::new(
                caret.x,
                line_top + BUTTON_H + LINE_GAP,
                content.right() - caret.x,
                SLIDER_HEIGHT,
            )
        });
        Self {
            bar,
            caret,
            name,
            toggles: [mute, solo],
            volume,
            meter,
        }
    }
}

/// A gain slider drag: the value follows the pointer's distance from where
/// it was anchored, as Tonique's `ParamSlider`.
#[derive(Clone, Copy)]
struct GainDrag {
    track: TrackId,
    /// The gain when the drag started, for undo.
    start: f32,
    anchor_x: f32,
    /// Slider value (dB) at `anchor_x`.
    anchor_db: f32,
    fine: bool,
    moved: bool,
}

#[derive(Default)]
pub struct HeadersUi {
    sliders: Rc<RefCell<HashMap<TrackId, Bounds<Pixels>>>>,
    drag: Option<GainDrag>,
}

/// The slider's position (dB) for a linear gain.
fn slider_db(gain: f32) -> f32 {
    if gain > 0. {
        (20. * gain.log10()).clamp(DB_MIN, DB_MAX)
    } else {
        DB_MIN
    }
}

/// The linear gain for a slider position: its bottom is silence.
fn gain_of(db: f32) -> f32 {
    if db <= DB_MIN {
        0.
    } else {
        10f32.powf(db / 20.)
    }
}

/// A linear gain in dB: `-3.2`, or `-inf` for silence.
fn format_db(gain: f32) -> String {
    if gain > 0. {
        format!("{:.1}", 20. * gain.log10())
    } else {
        "-inf".into()
    }
}

impl Tonique {
    fn track_volume(&self, id: TrackId) -> f32 {
        self.state
            .tracks()
            .find(|t| t.id == id)
            .map_or(1., |t| t.volume)
    }

    fn gain_down(&mut self, id: TrackId, ev: &MouseDownEvent) {
        let volume = self.track_volume(id);
        // Double-click: back to unity, one undo step.
        if ev.click_count >= 2 {
            self.headers.drag = None;
            if volume != 1. {
                self.state.set_volume(id, 1.);
                self.state.commit_volume(id, volume, 1.);
            }
            return;
        }
        self.headers.drag = Some(GainDrag {
            track: id,
            start: volume,
            anchor_x: f32::from(ev.position.x),
            anchor_db: slider_db(volume),
            fine: ev.modifiers.shift,
            moved: false,
        });
    }

    pub fn header_drag_move(&mut self, ev: &MouseMoveEvent) -> bool {
        let Some(mut drag) = self.headers.drag else {
            return false;
        };
        let Some(bounds) = self.headers.sliders.borrow().get(&drag.track).copied() else {
            return false;
        };
        let x = f32::from(ev.position.x);
        let fine = ev.modifiers.shift;
        // Toggling Shift re-anchors, so the value doesn't jump.
        if drag.fine != fine {
            drag.anchor_x = x;
            drag.anchor_db = slider_db(self.track_volume(drag.track));
            drag.fine = fine;
        }
        let speed = if drag.fine { FINE_DRAG } else { 1. };
        let per_px = (DB_MAX - DB_MIN) / f32::from(bounds.size.width).max(1.) * speed;
        let db = (drag.anchor_db + (x - drag.anchor_x) * per_px).clamp(DB_MIN, DB_MAX);
        let gain = gain_of(db);
        if gain != self.track_volume(drag.track) {
            self.state.set_volume(drag.track, gain);
            drag.moved = true;
        }
        self.headers.drag = Some(drag);
        true
    }

    pub fn header_drag_end(&mut self) {
        if let Some(drag) = self.headers.drag.take() {
            let volume = self.track_volume(drag.track);
            if drag.moved && volume != drag.start {
                self.state.commit_volume(drag.track, drag.start, volume);
            }
        }
    }

    fn header(&self, track: TrackRow, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let id = track.id;
        let layout = HeaderLayout::new(HEADERS_WIDTH, track.height, track.collapsed);
        let silenced = track.is_silenced();
        let fill = if track.selected {
            t.bg_control
        } else {
            t.bg_raised
        };
        let place = |r: R| {
            div()
                .absolute()
                .left(px(r.x))
                .top(px(r.y))
                .w(px(r.w))
                .h(px(r.h))
        };

        // Mute in the warning colour; solo in the track's (fainter when
        // silenced by another solo), as Tonique's toggles.
        let toggle = |label: &'static str, on: Option<Color>, r: R| {
            let el = place(r)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .text_size(px(size::SMALL));
            match on {
                Some(c) => el.bg(h(c)).text_color(h(t.text_on(c))),
                None => el
                    .border_1()
                    .border_color(h(t.border))
                    .text_color(h(t.text_muted)),
            }
            .hover(|s| s.opacity(0.85))
            .cursor_pointer()
            .child(label)
        };
        let solo_fill = match (matches!(track.solo, TrackSoloState::Solo), silenced) {
            (false, _) => None,
            (true, false) => Some(track.color),
            (true, true) => Some(with_alpha(track.color, 90)),
        };
        let mute = toggle("M", track.muted.then_some(t.warning), layout.toggles[0]).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                let muted = this
                    .state
                    .tracks()
                    .find(|t| t.id == id)
                    .is_some_and(|t| t.muted);
                this.state.set_mute(id, !muted);
                cx.notify();
            }),
        );
        let solo = toggle("S", solo_fill, layout.toggles[1]).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                // Shift: this one alone.
                this.state.toggle_solo(id, ev.modifiers.shift);
                cx.notify();
            }),
        );
        let collapsed = track.collapsed;
        let caret = place(layout.caret)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size::SMALL))
            .text_color(h(t.text_muted))
            .cursor_pointer()
            .child(icon_fill(if collapsed {
                icons::CARET_RIGHT
            } else {
                icons::CARET_DOWN
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.state.set_collapsed(&id, !collapsed);
                    cx.notify();
                }),
            );
        let name = place(layout.name)
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(size::LABEL))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(h(t.text))
            .child(display_name(&track.name, track.first_track_index));

        // The meter: what the track plays. Clicked, its clips are reset.
        let metrics = self.state.metrics.tracks.get(&id).cloned();
        let theme = t.clone();
        let meter = place(layout.meter)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.state.reset_meter_clip(id);
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        if let Some(m) = &metrics {
                            paint_meter(window, rect_of(b), m, &theme, silenced);
                        }
                    },
                )
                .size_full(),
            );

        let mut header = div()
            .id(("header", id.0 as usize))
            .relative()
            .flex_none()
            .w_full()
            .h(px(track.height))
            .mb(px(ROW_GAP))
            .bg(h(fill))
            .rounded(px(2.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    // Ctrl+click adds to the selection, Shift+click selects a range.
                    if ev.modifiers.control || ev.modifiers.platform {
                        this.state.toggle_track_selected(id);
                    } else if ev.modifiers.shift {
                        this.state.select_rows_to(id);
                    } else {
                        this.state.select_track(&id);
                    }
                    cx.notify();
                }),
            )
            .child(place(layout.bar).rounded(px(1.)).bg(h(if silenced {
                t.text_disabled
            } else {
                track.color
            })))
            .child(caret)
            .child(name)
            .child(mute)
            .child(solo)
            .child(meter);

        if let Some(volume) = layout.volume {
            let slot = self.headers.sliders.clone();
            let theme = t.clone();
            let (gain, color) = (track.volume, track.color);
            let suffix = volume.w >= VOLUME_UNIT_WIDTH;
            header = header.child(
                place(volume)
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.gain_down(id, ev);
                            cx.notify();
                        }),
                    )
                    .child(
                        canvas(
                            move |b, _, _| {
                                slot.borrow_mut().insert(id, b);
                            },
                            move |b, _, window, cx| {
                                paint_gain(window, cx, rect_of(b), gain, color, suffix, &theme)
                            },
                        )
                        .size_full(),
                    ),
            );
        }
        header.into_any_element()
    }

    pub fn headers_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        let offset = self.state.grid.offset.y;
        let mut list = div()
            .absolute()
            .top(px(RULER_H - offset))
            .left_0()
            .right_0()
            .flex()
            .flex_col();
        for row in rows(&self.state) {
            list = list.child(self.header(row.track, &t, cx));
        }

        div()
            .w(px(HEADERS_WIDTH))
            .h_full()
            .flex_none()
            .relative()
            .overflow_hidden()
            .bg(h(t.bg_deep))
            .child(list)
            // Over the list, beside the ruler.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(RULER_H))
                    .flex()
                    .items_end()
                    .px_2()
                    .pb_1()
                    .bg(h(t.bg_panel))
                    .border_b_1()
                    .border_color(h(t.separator))
                    .text_size(px(size::SMALL))
                    .text_color(h(t.text_muted))
                    .child(format!("{} tracks", self.state.track_count())),
            )
    }
}

fn rect_of(b: Bounds<Pixels>) -> R {
    R::new(
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.size.width),
        f32::from(b.size.height),
    )
}

/// The gain as Tonique's `ParamSlider` draws it in headers: a well, filled
/// in the track's colour from the left up to the level, the value on top in
/// ink that reads on the fill where it's over it and on the well elsewhere.
fn paint_gain(
    window: &mut Window,
    cx: &mut gpui::App,
    r: R,
    gain: f32,
    color: Color,
    suffix: bool,
    t: &Theme,
) {
    let db = slider_db(gain);
    let level = ((db - DB_MIN) / (DB_MAX - DB_MIN)).clamp(0., 1.);
    let filled = R::new(r.x, r.y, r.w * level, r.h);
    window.paint_quad(gpui::fill(r.bounds(), h(t.bg_deep)).corner_radii(px(1.)));
    window.paint_quad(gpui::fill(filled.bounds(), h(color)).corner_radii(px(1.)));

    let text = format!("{}{}", format_db(gain), if suffix { " dB" } else { "" });
    let shape = |window: &mut Window, ink: Color| {
        let run = TextRun {
            len: text.len(),
            font: gpui::font(FONT_UI),
            color: h(ink),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(text.clone().into(), px(size::SMALL), &[run], None)
    };
    let width = f32::from(shape(window, t.text).width);
    let at = point(
        px(r.x + (r.w - width) / 2.),
        px(r.y + (r.h - size::SMALL * 1.25) / 2.),
    );
    let well = [
        R::new(r.x, r.y, filled.x - r.x, r.h),
        R::new(filled.right(), r.y, r.right() - filled.right(), r.h),
    ];
    for (part, ink) in [
        (well[0], t.text),
        (well[1], t.text),
        (filled, t.text_on(color)),
    ] {
        if part.is_empty() {
            continue;
        }
        let line = shape(window, ink);
        window.with_content_mask(
            Some(gpui::ContentMask {
                bounds: part.bounds(),
            }),
            |window| {
                line.paint(
                    at,
                    px(size::SMALL * 1.25),
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
            },
        );
    }
}
