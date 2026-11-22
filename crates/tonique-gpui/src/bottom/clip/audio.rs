//! The inspector's Audio panel: how the clip sounds (gain, pan, its
//! transposition), and how it follows the project's tempo (its warp mode,
//! how it's stretched, its file's tempo).

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, MouseButton, MouseDownEvent, Pixels, Stateful,
    canvas, div, prelude::*, px,
};
use tonique_engine::edit::{ClipId, MAX_TRANSPOSE, StretchWindow, WarpMode};

use crate::{
    app::{ArrowHolder, PopupKind, Tonique},
    bottom::clip::{
        ClipArrows, ClipDrag,
        inspector::{
            CELL_GAP, FIELD_HEIGHT, Typed, cell, cells, section, toggle_section, value_box,
        },
    },
    core::{
        clip::{AudioClip, MAX_DETUNE, MIN_DETUNE, carried},
        state::ClipParam,
    },
    geometry::Rect,
    theme::{Color, Theme, hsla, size},
    utils::steps::Presses,
    widgets::{Flat, Menu, MenuItem, Scale, SliderDrag, paint_param_slider, select, typing_view},
};

/// The gain slider's range, in dB: its bottom is silence, its top the
/// engine's limit (+12 dB).
const CLIP_GAIN: Scale = Scale::Gain {
    min: -40.,
    max: 12.,
};
/// The transposition's big step, with Shift and an arrow key.
const OCTAVE: f32 = 12.;
/// The detune's big step, in cents.
const DETUNE_BIG_STEP: f32 = 10.;
/// The warp mode's picker, on its section's title row.
const WARP_MODE_WIDTH: f32 = 80.;
/// The file's tempo halved or doubled, beside it.
const TEMPO_BUTTON_WIDTH: f32 = 26.;
/// The file's tempo as typed: `120.00` and room around it.
const TEMPO_FIELD_WIDTH: f32 = 72.;

/// A slider of the panel's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipSlider {
    Gain,
    Pan,
    Transpose,
    Detune,
}

impl ClipSlider {
    fn scale(self) -> Scale {
        match self {
            Self::Gain => CLIP_GAIN,
            Self::Pan => Scale::Pan,
            Self::Transpose => Scale::Steps {
                min: -MAX_TRANSPOSE,
                max: MAX_TRANSPOSE,
                big: OCTAVE,
                unit: "st",
            },
            Self::Detune => Scale::Steps {
                min: MIN_DETUNE,
                max: MAX_DETUNE,
                big: DETUNE_BIG_STEP,
                unit: "ct",
            },
        }
    }

    fn value(self, clip: &AudioClip) -> f32 {
        match self {
            Self::Gain => clip.gain,
            Self::Pan => clip.pan,
            Self::Transpose => clip.transpose as f32,
            Self::Detune => clip.detune,
        }
    }

    fn param(self) -> Option<ClipParam> {
        match self {
            Self::Gain => Some(ClipParam::Gain),
            Self::Pan => Some(ClipParam::Pan),
            Self::Transpose | Self::Detune => None,
        }
    }
}

fn window_name(window: StretchWindow) -> &'static str {
    match window {
        StretchWindow::Short => "Short",
        StretchWindow::Normal => "Normal",
        StretchWindow::Long => "Long",
    }
}

impl Tonique {
    /// Gain and pan, sliders like a track's heard as they're dragged, and
    /// the transposition and detune alike; the warp, switched by its title,
    /// its mode beside it; the stretch's window and formants; the file's
    /// tempo, typed or halved and doubled.
    pub(super) fn audio_panel(
        &self,
        shown: &AudioClip,
        color: Color,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let slider = |this: &Self, label, slider, context: &mut Context<Self>| {
            cell(
                label,
                this.clip_slider(slider, shown, color, theme, context),
                true,
                theme,
            )
        };
        let id = shown.id;
        // Off, it shows how it would play, greyed out.
        let warped = shown.warp != WarpMode::Off;
        let warp = toggle_section(
            "warp-toggle",
            "Warp",
            warped,
            true,
            theme,
            context,
            move |this| {
                this.edit_warp(id, |clip, bpm| {
                    let warp = if clip.warp == WarpMode::Off {
                        WarpMode::Spectral
                    } else {
                        WarpMode::Off
                    };
                    clip.set_warp(warp, clip.bpm, clip.transpose, clip.detune, bpm);
                });
            },
        )
        .child(self.picker(
            "warp-mode",
            "Tonal",
            PopupKind::ClipWarpMode(id),
            warped,
            WARP_MODE_WIDTH,
            theme,
            context,
        ));
        let window_picker = self.picker(
            "stretch-window",
            window_name(shown.stretch.window),
            PopupKind::ClipWindow(id),
            warped,
            0.,
            theme,
            context,
        );
        let formants = shown.stretch.formants;
        let formants = Flat::new("formants", FIELD_HEIGHT)
            .full_width()
            .selected(formants)
            .enabled(warped)
            .show(
                if formants { "Keep" } else { "Move" },
                theme,
                context,
                move |this, _| {
                    this.edit_warp(id, |clip, _| clip.stretch.formants ^= true);
                },
            );
        div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(section("Audio", theme))
            .child(cells(
                slider(self, "Gain", ClipSlider::Gain, context),
                slider(self, "Pan", ClipSlider::Pan, context),
            ))
            .child(div().h(px(CELL_GAP)))
            .child(cells(
                slider(self, "Transpose", ClipSlider::Transpose, context),
                slider(self, "Detune", ClipSlider::Detune, context),
            ))
            .child(warp)
            .child(cells(
                cell("Window", window_picker, warped, theme),
                cell("Formants", formants, warped, theme),
            ))
            .child(div().h(px(CELL_GAP)))
            .child(cell(
                "BPM",
                self.tempo_row(shown, warped, theme, context),
                warped,
                theme,
            ))
            .into_any_element()
    }

    /// A dropdown opening `kind`'s menu, `width` wide (its cell's when 0);
    /// greyed out and inert unless `enabled`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn picker(
        &self,
        id: &'static str,
        label: &'static str,
        kind: PopupKind,
        enabled: bool,
        width: f32,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let open = self.popup.as_ref().is_some_and(|popup| popup.kind == kind);
        let element = if enabled {
            select(id, label, width.max(1.), kind, open, theme, context)
        } else {
            div()
                .id(id)
                .flex()
                .items_center()
                .px(px(6.))
                .rounded(px(2.))
                .border_1()
                .border_color(hsla(theme.border))
                .bg(hsla(theme.bg_control))
                .opacity(0.5)
                .child(label)
        };
        let element = element.h(px(FIELD_HEIGHT)).text_size(px(size::LABEL));
        if width > 0. {
            element.flex_none().w(px(width))
        } else {
            element.w_full()
        }
    }

    /// The file's tempo, typed and applied on Enter (each change resizes
    /// the clip), or halved and doubled.
    fn tempo_row(
        &self,
        shown: &AudioClip,
        enabled: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let id = shown.id;
        let typing = match &self.bottom.clip.typing {
            Some((Typed::Bpm, text)) => Some(text),
            _ => None,
        };
        let field = match typing {
            Some(text) => typing_view(
                "typing",
                text,
                theme,
                context,
                |this| this.bottom.clip.typing.as_mut().map(|(_, text)| text),
                Self::finish_typing,
            )
            .h(px(FIELD_HEIGHT))
            .into_any_element(),
            None => {
                let element = value_box("bpm", format!("{:.2}", shown.bpm), enabled, theme)
                    .cursor(CursorStyle::IBeam);
                if enabled {
                    element
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(move |this, _, window, context| {
                                context.stop_propagation();
                                if let Some((_, clip)) = this.state.clip_with_track(id) {
                                    this.type_bpm(&clip, window, context);
                                }
                                context.notify();
                            }),
                        )
                        .into_any_element()
                } else {
                    element.into_any_element()
                }
            }
        };
        let button = |label: &'static str, factor: f64, context: &mut Context<Self>| {
            Flat::new(label, FIELD_HEIGHT)
                .width(TEMPO_BUTTON_WIDTH)
                .enabled(enabled)
                .show(label, theme, context, move |this, _| {
                    let Some((_, clip)) = this.state.clip_with_track(id) else {
                        return;
                    };
                    let bpm = this.state.bpm();
                    let mut edited = clip.clone();
                    edited.set_warp(
                        clip.warp,
                        clip.bpm * factor,
                        clip.transpose,
                        clip.detune,
                        bpm,
                    );
                    this.state.commit_resize_clip(edited);
                })
        };
        div()
            .flex()
            .gap(px(CELL_GAP))
            .child(div().flex_none().w(px(TEMPO_FIELD_WIDTH)).child(field))
            .child(button("÷2", 0.5, context))
            .child(button("×2", 2., context))
    }

    /// Clip `id` edited by `edit` at once, heard, as one undo step.
    fn edit_warp(&mut self, id: ClipId, edit: impl FnOnce(&mut AudioClip, f32)) {
        let Some((_, clip)) = self.state.clip_with_track(id) else {
            return;
        };
        let mut edited = clip.clone();
        edit(&mut edited, self.state.bpm());
        self.state.edit_clip_live(edited, true);
    }

    /// The slider of `slider`, in the clip's colour: dragged, heard as it
    /// moves and one undo step once released; double-clicked, back to its
    /// default; clicked, stepped by the arrow keys.
    fn clip_slider(
        &self,
        slider: ClipSlider,
        shown: &AudioClip,
        color: Color,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let value = slider.value(shown);
        let scale = slider.scale();
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let slot = bounds.clone();
        let paint_theme = theme.clone();
        let id = shown.id;
        let holder = ArrowHolder::Clip(ClipArrows::Slider(slider));
        let focused = self.holds_arrows(&holder);
        div()
            .id(("clip-slider", slider as usize))
            .w_full()
            .h(px(FIELD_HEIGHT))
            .cursor(CursorStyle::ResizeLeftRight)
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, context| {
                        paint_param_slider(
                            window,
                            context,
                            Rect::of(bounds),
                            scale,
                            value,
                            color,
                            (true, focused),
                            &paint_theme,
                        )
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
                        this.bottom.clip.drag = None;
                        if let Some((_, clip)) = this.state.clip_with_track(id) {
                            this.set_slider(&clip, slider, scale.default_value(), Some(value));
                        }
                    } else {
                        let pointer_x = f32::from(event.position.x);
                        this.bottom.clip.drag = Some(ClipDrag::Slider {
                            slider,
                            drag: SliderDrag::new(scale, value, pointer_x, event.modifiers.shift),
                            bounds: bounds.get(),
                        });
                    }
                    context.notify();
                }),
            )
    }

    /// Sets `slider` of `clip` to `value`: live, or as an undo step from
    /// `done`'s value when it's given.
    fn set_slider(&mut self, clip: &AudioClip, slider: ClipSlider, value: f32, done: Option<f32>) {
        if let Some(param) = slider.param() {
            match done {
                Some(start) => self.state.commit_clip_param(clip.id, param, start, value),
                None => self.state.set_clip_param(clip.id, param, value),
            }
            return;
        }
        let bpm = self.state.bpm();
        let (transpose, detune) = match slider {
            ClipSlider::Transpose => (value as i32, clip.detune),
            _ => (clip.transpose, value),
        };
        let mut edited = clip.clone();
        edited.set_warp(clip.warp, clip.bpm, transpose, detune, bpm);
        self.state.edit_clip_live(edited, done.is_some());
    }

    /// `slider` stepped by `presses` of the arrow keys, as one undo step;
    /// the detune carries over into the transposition.
    pub(super) fn step_clip_slider(
        &mut self,
        clip: &AudioClip,
        slider: ClipSlider,
        presses: Presses,
    ) {
        let value = slider.value(clip);
        let scale = slider.scale();
        if slider == ClipSlider::Detune {
            let (transpose, detune) = carried(clip.transpose, scale.carried(value, presses));
            if (transpose, detune) != (clip.transpose, clip.detune) {
                let mut edited = clip.clone();
                edited.set_warp(clip.warp, clip.bpm, transpose, detune, self.state.bpm());
                self.state.edit_clip_live(edited, true);
            }
            return;
        }
        let stepped = scale.stepped(value, presses);
        if stepped != value {
            self.set_slider(clip, slider, stepped, Some(value));
        }
    }

    pub(super) fn follow_slider(&mut self, clip: &AudioClip, pointer_x: f32, fine: bool) {
        let Some(ClipDrag::Slider {
            slider,
            drag,
            bounds,
        }) = self.bottom.clip.drag.as_mut()
        else {
            return;
        };
        let (slider, width) = (*slider, f32::from(bounds.size.width));
        let current = slider.value(clip);
        if let Some(value) = drag.follow(pointer_x, fine, width, current) {
            self.set_slider(clip, slider, value, None);
        }
    }

    pub(super) fn end_slider(&mut self, clip: &AudioClip, slider: ClipSlider, drag: SliderDrag) {
        if drag.moved {
            self.set_slider(clip, slider, slider.value(clip), Some(drag.start));
        }
    }

    /// The warp modes: stretching (tonal) is the only one yet.
    pub fn clip_warp_menu(&self, id: ClipId, context: &mut Context<Self>) -> Option<gpui::Div> {
        let (_, clip) = self.state.clip_with_track(id)?;
        Some(
            Menu::new(&self.theme)
                .item(
                    MenuItem::new("Tonal").checked(clip.warp == WarpMode::Spectral),
                    context,
                    move |this, _, _| {
                        this.edit_warp(id, |clip, bpm| {
                            clip.set_warp(
                                WarpMode::Spectral,
                                clip.bpm,
                                clip.transpose,
                                clip.detune,
                                bpm,
                            )
                        })
                    },
                )
                .into_element(),
        )
    }

    /// The stretch's windows.
    pub fn clip_window_menu(&self, id: ClipId, context: &mut Context<Self>) -> Option<gpui::Div> {
        let (_, clip) = self.state.clip_with_track(id)?;
        let mut menu = Menu::new(&self.theme);
        for window in StretchWindow::ALL {
            menu = menu.item(
                MenuItem::new(window_name(window)).checked(clip.stretch.window == window),
                context,
                move |this, _, _| this.edit_warp(id, |clip, _| clip.stretch.window = window),
            );
        }
        Some(menu.into_element())
    }
}
