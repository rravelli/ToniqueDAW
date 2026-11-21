//! Waveforms, as `ui/waveform.rs` draws them in the egui app: one column
//! per physical pixel, anchored to the start of the file so scrolling
//! doesn't make them jitter, read from the peak pyramid (or the samples
//! when zoomed in), and cached by tiles of columns.
//!
//! egui caches each tile as a mesh; here each tile is drawn once into an
//! image, uploaded to the GPU once and then only placed, one sprite a tile.

use std::{collections::HashMap, ops::Range, sync::Arc};

use gpui::{Bounds, Corners, Hsla, RenderImage, Rgba, Window, point, px, size};
use tonique_engine::{
    nodes::balance,
    peaks::{BASE_BUCKET, WaveformPeaks},
    sample::SampleBuffer,
};

use crate::{analysis::AudioData, core::clip::AudioClip};

/// Columns per cached tile.
const TILE: usize = 256;
/// Below this many samples per column, read the samples themselves.
const RAW_MAX_SPP: f64 = (4 * BASE_BUCKET) as f64;
const EMPTY: [f32; 2] = [f32::NAN; 2];

/// A rectangle in window coordinates (logical pixels).
#[derive(Clone, Copy, Debug)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn intersect(&self, o: R) -> R {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        R::new(
            x,
            y,
            (self.right().min(o.right()) - x).max(0.),
            (self.bottom().min(o.bottom()) - y).max(0.),
        )
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0. || self.h <= 0.
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn bounds(&self) -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(self.x), px(self.y)), size(px(self.w), px(self.h)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TileKey {
    peaks: usize,
    raw: bool,
    start: u64,
    spp: u64,
    lane: (usize, usize),
    tile: usize,
}

/// What a tile's image depends on besides its columns: its height, the
/// lane's gain and the colour.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ImageKey {
    tile: TileKey,
    height: u32,
    gain: u32,
    color: [u8; 4],
}

/// A tile's column values, and the frame it was last drawn in.
type Tile = (Arc<Vec<[f32; 2]>>, u64);

/// Tiles of waveform drawn lately: their column values, and their images
/// (uploaded to the GPU once, then only placed). Those unused for a frame
/// go, their images taken off the GPU.
#[derive(Default)]
pub struct WaveCache {
    frame: u64,
    tiles: HashMap<TileKey, Tile>,
    images: HashMap<ImageKey, (Arc<RenderImage>, u64)>,
    /// Keeps the peak snapshots the keys point to alive.
    peaks: HashMap<usize, (Arc<WaveformPeaks>, u64)>,
}

impl WaveCache {
    pub fn start_frame(&mut self, window: &mut Window) {
        self.frame += 1;
        let frame = self.frame;
        self.tiles.retain(|_, (_, used)| *used + 1 >= frame);
        self.peaks.retain(|_, (_, used)| *used + 1 >= frame);
        self.images.retain(|_, (image, used)| {
            let keep = *used + 1 >= frame;
            if !keep {
                window.drop_image(image.clone()).ok();
            }
            keep
        });
    }
}

struct Lane {
    channels: Range<usize>,
    top: f32,
    height: f32,
    gain: f32,
}

fn lanes(rect: R, stereo: bool, channels: usize, gain: f32, pan: f32) -> Vec<Lane> {
    let (left, right) = balance(pan);
    if !stereo {
        return vec![Lane {
            channels: 0..channels,
            top: rect.y,
            height: rect.h,
            gain: gain * (left + right) / 2.,
        }];
    }
    let height = rect.h / channels as f32;
    (0..channels)
        .map(|ch| Lane {
            channels: ch..ch + 1,
            top: rect.y + ch as f32 * height,
            height,
            gain: gain
                * match ch {
                    0 => left,
                    1 => right,
                    _ => 1.,
                },
        })
        .collect()
}

/// Draws the samples `samples` of `data` stretched over `file`, only
/// inside `visible`.
#[allow(clippy::too_many_arguments)]
pub fn paint_waveform(
    window: &mut Window,
    cache: &mut WaveCache,
    file: R,
    visible: R,
    data: &AudioData,
    samples: Range<f64>,
    stereo: bool,
    color: Hsla,
    gain: f32,
    pan: f32,
) {
    let peaks = data.peaks();
    let channels = peaks.num_channels();
    if channels == 0 || file.w <= 0. || samples.end <= samples.start || visible.is_empty() {
        return;
    }
    let ppp = window.scale_factor() as f64;
    let origin = (file.x as f64 * ppp).round();
    let count = (file.w as f64 * ppp).ceil() as usize;
    let column = |x: f32| (x as f64 * ppp - origin).clamp(0., count as f64);
    let spp = (samples.end - samples.start) / (file.w as f64 * ppp);
    let shown = column(visible.x).floor() as usize..column(visible.right()).ceil() as usize;
    let to_x = |c: f64| ((origin + c) / ppp) as f32;
    let raw = data.samples().map(|s| s.as_ref());
    let lanes = lanes(file, stereo, channels, gain, pan);
    let rgba = Rgba::from(color);
    let color = [rgba.r, rgba.g, rgba.b, rgba.a].map(|v| (v.clamp(0., 1.) * 255.).round() as u8);

    let peaks_id = Arc::as_ptr(&peaks) as usize;
    let frame = cache.frame;
    cache.peaks.insert(peaks_id, (peaks.clone(), frame));

    for lane in &lanes {
        let lane_rect = R::new(visible.x, lane.top, visible.w, lane.height).intersect(visible);
        if lane_rect.is_empty() {
            continue;
        }
        let height = (lane.height as f64 * ppp).round().max(1.) as u32;
        for tile in shown.start / TILE..shown.end.div_ceil(TILE) {
            let key = TileKey {
                peaks: peaks_id,
                raw: raw.is_some(),
                start: samples.start.to_bits(),
                spp: spp.to_bits(),
                lane: (lane.channels.start, lane.channels.end),
                tile,
            };
            let image_key = ImageKey {
                tile: key,
                height,
                gain: lane.gain.to_bits(),
                color,
            };
            let image = match cache.images.get_mut(&image_key) {
                Some((image, used)) => {
                    *used = frame;
                    image.clone()
                }
                None => {
                    let values = match cache.tiles.get_mut(&key) {
                        Some((values, used)) => {
                            *used = frame;
                            values.clone()
                        }
                        None => {
                            let values: Arc<Vec<[f32; 2]>> = Arc::new(
                                (tile * TILE..((tile + 1) * TILE).min(count))
                                    .map(|c| {
                                        column_values(
                                            &peaks,
                                            raw,
                                            samples.start,
                                            spp,
                                            &lane.channels,
                                            c,
                                        )
                                    })
                                    .collect(),
                            );
                            cache.tiles.insert(key, (values.clone(), frame));
                            values
                        }
                    };
                    let Some(image) = rasterize(&values, height, lane.gain, color) else {
                        continue;
                    };
                    cache.images.insert(image_key, (image.clone(), frame));
                    image
                }
            };
            let columns = image.size(0).width.0 as f64;
            let tile_rect = R::new(
                to_x((tile * TILE) as f64),
                lane.top,
                (columns / ppp) as f32,
                height as f32 / ppp as f32,
            );
            let shown = tile_rect.intersect(lane_rect);
            if !shown.is_empty() {
                window
                    .paint_image(
                        shown.bounds(),
                        tile_rect.bounds(),
                        Corners::default(),
                        image,
                        0,
                        false,
                    )
                    .ok();
            }
        }
    }
}

/// A tile's columns drawn into an image `height` pixels tall: each column
/// filled from its max to its min (at least a pixel), its ends partly
/// covered, in `color` (straight alpha, BGRA as gpui takes it).
fn rasterize(
    values: &[[f32; 2]],
    height: u32,
    gain: f32,
    [r, g, b, a]: [u8; 4],
) -> Option<Arc<RenderImage>> {
    let width = values.len() as u32;
    if width == 0 {
        return None;
    }
    let h = height as f32;
    let y = |v: f32| h / 2. * (1. - (v * gain).clamp(-1., 1.));
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (x, &[min, max]) in values.iter().enumerate() {
        if min.is_nan() {
            continue;
        }
        let top = y(max);
        let bottom = y(min).max(top + 1.).min(h);
        let first = top.floor().max(0.) as u32;
        let last = (bottom.ceil() as u32).min(height);
        for row in first..last {
            // How much of this pixel the column covers.
            let cover = ((row + 1) as f32).min(bottom) - (row as f32).max(top);
            let i = ((row * width + x as u32) * 4) as usize;
            pixels[i..i + 4].copy_from_slice(&[
                b,
                g,
                r,
                (a as f32 * cover.clamp(0., 1.)).round() as u8,
            ]);
        }
    }
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// `[min, max]` of the lane's channels under column `c`: from the samples
/// when zoomed in (the samples either side of a column, so they read as a
/// line past a sample per pixel), from the peak pyramid otherwise.
fn column_values(
    peaks: &WaveformPeaks,
    raw: Option<&SampleBuffer>,
    start: f64,
    spp: f64,
    channels: &Range<usize>,
    c: usize,
) -> [f32; 2] {
    let a = (start + c as f64 * spp).floor().max(0.) as usize;
    let b = if spp < 1. {
        (start + (c + 1) as f64 * spp).ceil() as usize + 1
    } else {
        ((start + (c + 1) as f64 * spp).floor() as usize).max(a + 1)
    };
    channels
        .clone()
        .filter_map(|ch| match raw {
            Some(raw) if spp < RAW_MAX_SPP => peaks.range_exact(raw, ch, a, b),
            _ => peaks.range_approx(ch, a, b, (spp / 4.) as usize),
        })
        .reduce(|x, y| (x.0.min(y.0), x.1.max(y.1)))
        .map_or(EMPTY, |(min, max)| [min, max])
}

/// The waveform of `clip` over `rect` (from the clip's start), pass after
/// pass of its loop, as `paint_clip_waveform` in the egui app.
#[allow(clippy::too_many_arguments)]
pub fn paint_clip_waveform(
    window: &mut Window,
    cache: &mut WaveCache,
    clip: &AudioClip,
    rect: R,
    viewport: R,
    pixels_per_beat: f32,
    bpm: f32,
    color: Hsla,
) {
    let Some(frames) = clip.audio.total_frames() else {
        return;
    };
    let seconds = clip.source_seconds() as f64;
    if seconds <= 0. {
        return;
    }
    let x = |beats: f64| rect.x + beats as f32 * pixels_per_beat;
    let file_width = clip.source_beats(bpm) as f32 * pixels_per_beat;
    let beats = |px: f32| ((px - rect.x) / pixels_per_beat) as f64;
    let in_view = beats(viewport.x).max(0.)..beats(viewport.right());
    for segment in clip.segments(in_view, bpm) {
        let (left, right) = (x(segment.start), x(segment.end));
        if right < viewport.x || left > viewport.right() {
            continue;
        }
        let file_left = left - (segment.file_start / seconds) as f32 * file_width;
        let file = R::new(file_left, rect.y, file_width, rect.h);
        let shown = R::new(left, rect.y, right - left, rect.h)
            .intersect(rect)
            .intersect(viewport);
        crate::profile::time("paint.waveforms", || {
            paint_waveform(
                window,
                cache,
                file,
                shown,
                &clip.audio.data,
                0.0..frames,
                clip.audio.channels >= 2,
                color,
                clip.gain,
                clip.pan,
            )
        });
        if segment.start > 0. && left >= viewport.x && left <= viewport.right() {
            window.paint_quad(gpui::fill(
                R::new(left, rect.y, 1., rect.h).bounds(),
                color.opacity(0.43),
            ));
        }
    }
}
