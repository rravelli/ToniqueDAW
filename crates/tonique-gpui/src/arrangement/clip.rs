//! A clip as `ClipView::paint` draws it: its body a tint of its track's
//! colour with its waveform, its header solid, its name on it.

use gpui::{ContentMask, Window, px};

use crate::{
    arrangement::{rows::HEADER_HEIGHT, scene::SceneClip},
    geometry::Rect,
    icons,
    theme::{
        BLACK, Color, Theme, WHITE, contrast, hsla, icon_font, mix, mono, over, size, with_alpha,
    },
    wave::{WaveCache, paint_clip_waveform},
    widgets::paint_text,
};

/// Between a clip and the next, so cuts show.
const CUT_GAP: f32 = 1.;
const BODY_TINT: f32 = 0.7;
const SELECTION_FILL: f32 = 0.8;
const COLLAPSED_WAVE: f32 = 0.4;
const SILENCED_BODY: f32 = 0.16;
const SILENCED_HEADER: f32 = 0.27;
const SILENCED_BORDER: f32 = 0.43;
const SILENCED_WAVE: f32 = 0.5;
/// The contrast black needs on a body to draw its waveform in.
const WAVE_CONTRAST: f32 = 3.;
/// How opaque the clip a drop would add is.
const GHOST_ALPHA: u8 = 170;
/// Room a name needs to be drawn at all.
const NAME_MIN_WIDTH: f32 = 12.;

/// What painting clips needs.
pub struct ClipPaint<'frame> {
    pub window: &'frame mut Window,
    pub context: &'frame mut gpui::App,
    pub waves: &'frame mut WaveCache,
    pub theme: &'frame Theme,
    /// The lanes, in the window.
    pub lanes: Rect,
    /// The timeline's top left, in the window.
    pub origin: (f32, f32),
    pub pixels_per_beat: f32,
    pub bpm: f32,
}

/// A clip's colours.
struct Look {
    body: Color,
    /// Its selected part's body.
    selection: Color,
    header: Color,
    ink: Color,
    outline: Color,
    wave: Color,
    selected_wave: Color,
}

impl Look {
    fn new(scene_clip: &SceneClip, theme: &Theme) -> Self {
        let color = scene_clip.color;
        let tint = |amount: f32| mix(theme.bg_base, color, amount);
        let dim = scene_clip.silenced || scene_clip.clip.audio.missing;
        let (body, header, ink, outline) = if dim {
            (
                tint(SILENCED_BODY),
                tint(SILENCED_HEADER),
                theme.text_muted,
                tint(SILENCED_BORDER),
            )
        } else {
            (tint(BODY_TINT), color, theme.text_on(color), color)
        };
        let selection = mix(color, theme.clip_selected, SELECTION_FILL);
        // The waveform: black, or white on what's too dark for it; a wash
        // of the colour on silenced clips; black on what a drag leaves.
        let wave_on = |background: Color| {
            let wave = if scene_clip.silenced {
                tint(SILENCED_WAVE)
            } else if scene_clip.left_behind || contrast(BLACK, background) >= WAVE_CONTRAST {
                BLACK
            } else {
                WHITE
            };
            if scene_clip.collapsed {
                mix(background, wave, COLLAPSED_WAVE)
            } else {
                wave
            }
        };
        let header = if scene_clip.lit {
            over(header, theme.hover_overlay)
        } else {
            header
        };
        Self {
            body,
            selection,
            header,
            ink,
            outline,
            wave: wave_on(body),
            selected_wave: wave_on(selection),
        }
    }

    /// Faded, for the clip a drop would add.
    fn ghost(self) -> Self {
        let fade = |color: Color| with_alpha(color, GHOST_ALPHA);
        Self {
            body: fade(self.body),
            header: fade(self.header),
            wave: fade(self.wave),
            ..self
        }
    }
}

pub fn paint_clip(painter: &mut ClipPaint, scene_clip: &SceneClip, ghost: bool) {
    let (origin_x, origin_y) = painter.origin;
    let mut rect = Rect::new(
        origin_x + scene_clip.rect.left,
        origin_y + scene_clip.rect.top,
        scene_clip.rect.width,
        scene_clip.rect.height,
    );
    if rect.width > 2. * CUT_GAP {
        rect.width -= CUT_GAP;
    }
    if rect.right() < painter.lanes.left || rect.left > painter.lanes.right() {
        return;
    }
    let look = Look::new(scene_clip, painter.theme);
    let look = if ghost { look.ghost() } else { look };

    painter.window.paint_quad(gpui::quad(
        rect.bounds(),
        px(2.),
        hsla(look.body),
        px(1.),
        hsla(look.outline),
        gpui::BorderStyle::Solid,
    ));
    let wave = Rect::new(
        rect.left,
        rect.top + HEADER_HEIGHT,
        rect.width,
        rect.height - HEADER_HEIGHT,
    );
    if wave.height > 2. {
        let clip_rect = Rect::new(
            origin_x + scene_clip.rect.left,
            wave.top,
            scene_clip.rect.width,
            wave.height,
        );
        let shown = wave.intersect(painter.lanes);
        paint_clip_waveform(
            painter.window,
            painter.waves,
            &scene_clip.clip,
            clip_rect,
            shown,
            painter.pixels_per_beat,
            painter.bpm,
            hsla(look.wave),
        );
        // The selected part: lit inside the outline, its waveform drawn
        // again over it.
        if let Some((start, stop)) = scene_clip.selected {
            let inside = Rect::new(rect.left + 1., wave.top, rect.width - 2., wave.height - 1.);
            let lit = Rect::new(origin_x + start, wave.top, stop - start, wave.height)
                .intersect(inside)
                .intersect(shown);
            if !lit.is_empty() {
                painter
                    .window
                    .paint_quad(gpui::fill(lit.bounds(), hsla(look.selection)));
                let (window, waves) = (&mut *painter.window, &mut *painter.waves);
                let mask = ContentMask {
                    bounds: lit.bounds(),
                };
                window.with_content_mask(Some(mask), |window| {
                    paint_clip_waveform(
                        window,
                        waves,
                        &scene_clip.clip,
                        clip_rect,
                        lit,
                        painter.pixels_per_beat,
                        painter.bpm,
                        hsla(look.selected_wave),
                    );
                });
            }
        }
    }
    let header = Rect::new(
        rect.left,
        rect.top,
        rect.width,
        HEADER_HEIGHT.min(rect.height),
    );
    painter
        .window
        .paint_quad(
            gpui::fill(header.bounds(), hsla(look.header)).corner_radii(gpui::Corners {
                top_left: px(2.),
                top_right: px(2.),
                bottom_left: px(0.),
                bottom_right: px(0.),
            }),
        );
    paint_name(painter, scene_clip, rect, look.ink);
    // Still loading: shaded.
    let missing = scene_clip.clip.audio.missing;
    if !scene_clip.clip.playable && !missing {
        painter.window.paint_quad(gpui::fill(
            wave.intersect(painter.lanes).bounds(),
            hsla(painter.theme.shadow),
        ));
    }
}

/// The name, kept in view when the clip starts off screen; after a warning
/// when its file is missing, as in Tonique.
fn paint_name(painter: &mut ClipPaint, scene_clip: &SceneClip, rect: Rect, ink: Color) {
    let name_x = rect.left.max(painter.lanes.left) + 4.;
    if rect.right() - name_x <= NAME_MIN_WIDTH {
        return;
    }
    let mask = Rect::new(rect.left, rect.top, rect.width - 2., HEADER_HEIGHT).bounds();
    let (theme, context) = (painter.theme, &mut *painter.context);
    painter
        .window
        .with_content_mask(Some(ContentMask { bounds: mask }), |window| {
            let mut name_left = name_x;
            let name_top = rect.top + 2.;
            if scene_clip.clip.audio.missing {
                let warning = hsla(theme.warning);
                let font = icon_font(false);
                name_left += paint_text(
                    window,
                    context,
                    icons::WARNING,
                    name_left,
                    name_top,
                    size::SMALL,
                    warning,
                    font,
                ) + 3.;
            }
            let name = scene_clip.clip.audio.name.clone();
            paint_text(
                window,
                context,
                name,
                name_left,
                name_top,
                size::SMALL,
                hsla(ink),
                mono(),
            );
        });
}
