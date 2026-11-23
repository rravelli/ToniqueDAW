//! Waveforms, as `ui/waveform.rs` draws them in the egui app: one column
//! per physical pixel, anchored to the start of the file so scrolling
//! doesn't make them jitter, read from the peak pyramid (or the samples
//! when zoomed in), and cached by tiles of columns.
//!
//! egui caches each tile as a mesh; here each tile is drawn once into an
//! image, uploaded to the GPU once and then only placed, one sprite a tile.

use std::{collections::HashMap, ops::Range, sync::Arc};

use gpui::{Corners, Hsla, RenderImage, Rgba, Window};
use tonique_engine::{
    nodes::balance,
    peaks::{BASE_BUCKET, WaveformPeaks},
    sample::SampleBuffer,
};

use crate::{analysis::AudioData, core::clip::AudioClip, geometry::Rect};

/// Columns per cached tile.
const TILE: usize = 256;
/// Below this many samples per column, read the samples themselves.
const RAW_MAX_SAMPLES_PER_COLUMN: f64 = (4 * BASE_BUCKET) as f64;
const EMPTY: [f32; 2] = [f32::NAN; 2];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TileKey {
    peaks: usize,
    raw: bool,
    start: u64,
    samples_per_column: u64,
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

/// What a [`WaveCache`] holds.
#[derive(Clone, Copy, Default)]
pub struct WaveMemory {
    pub tiles: usize,
    pub tile_bytes: usize,
    /// On the GPU.
    pub images: usize,
    pub image_bytes: usize,
}

impl WaveMemory {
    /// Both caches' together.
    pub fn add(self, other: Self) -> Self {
        Self {
            tiles: self.tiles + other.tiles,
            tile_bytes: self.tile_bytes + other.tile_bytes,
            images: self.images + other.images,
            image_bytes: self.image_bytes + other.image_bytes,
        }
    }
}

impl WaveCache {
    pub fn memory(&self) -> WaveMemory {
        let image_bytes = |image: &RenderImage| {
            let size = image.size(0);
            size.width.0 as usize * size.height.0 as usize * 4
        };
        WaveMemory {
            tiles: self.tiles.len(),
            tile_bytes: self
                .tiles
                .values()
                .map(|(values, _)| values.capacity() * size_of::<[f32; 2]>())
                .sum(),
            images: self.images.len(),
            image_bytes: self
                .images
                .values()
                .map(|(image, _)| image_bytes(image))
                .sum(),
        }
    }

    /// Lets go of the tiles not drawn since the last frame: called as the
    /// view drawing them renders, its canvases painting them again after.
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

fn lanes(rect: Rect, stereo: bool, channels: usize, gain: f32, pan: f32) -> Vec<Lane> {
    let (left, right) = balance(pan);
    if !stereo {
        return vec![Lane {
            channels: 0..channels,
            top: rect.top,
            height: rect.height,
            gain: gain * (left + right) / 2.,
        }];
    }
    let height = rect.height / channels as f32;
    (0..channels)
        .map(|channel| Lane {
            channels: channel..channel + 1,
            top: rect.top + channel as f32 * height,
            height,
            gain: gain
                * match channel {
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
    file: Rect,
    visible: Rect,
    data: &AudioData,
    samples: Range<f64>,
    stereo: bool,
    color: Hsla,
    gain: f32,
    pan: f32,
) {
    let peaks = data.peaks();
    let channels = peaks.num_channels();
    if channels == 0 || file.width <= 0. || samples.end <= samples.start || visible.is_empty() {
        return;
    }
    let pixels_per_point = window.scale_factor() as f64;
    let origin = (file.left as f64 * pixels_per_point).round();
    let count = (file.width as f64 * pixels_per_point).ceil() as usize;
    let column =
        |window_x: f32| (window_x as f64 * pixels_per_point - origin).clamp(0., count as f64);
    let samples_per_column = (samples.end - samples.start) / (file.width as f64 * pixels_per_point);
    let shown = column(visible.left).floor() as usize..column(visible.right()).ceil() as usize;
    let column_left = |column_index: f64| ((origin + column_index) / pixels_per_point) as f32;
    let raw = data.samples().map(|buffer| buffer.as_ref());
    let lanes = lanes(file, stereo, channels, gain, pan);
    let rgba = Rgba::from(color);
    let color = [rgba.r, rgba.g, rgba.b, rgba.a]
        .map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8);

    let peaks_id = Arc::as_ptr(&peaks) as usize;
    let frame = cache.frame;
    cache.peaks.insert(peaks_id, (peaks.clone(), frame));

    for lane in &lanes {
        let lane_rect =
            Rect::new(visible.left, lane.top, visible.width, lane.height).intersect(visible);
        if lane_rect.is_empty() {
            continue;
        }
        let height = (lane.height as f64 * pixels_per_point).round().max(1.) as u32;
        for tile in shown.start / TILE..shown.end.div_ceil(TILE) {
            let key = TileKey {
                peaks: peaks_id,
                raw: raw.is_some(),
                start: samples.start.to_bits(),
                samples_per_column: samples_per_column.to_bits(),
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
                                    .map(|column_index| {
                                        column_values(
                                            &peaks,
                                            raw,
                                            samples.start,
                                            samples_per_column,
                                            &lane.channels,
                                            column_index,
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
            let tile_rect = Rect::new(
                column_left((tile * TILE) as f64),
                lane.top,
                (columns / pixels_per_point) as f32,
                height as f32 / pixels_per_point as f32,
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
    [red, green, blue, alpha]: [u8; 4],
) -> Option<Arc<RenderImage>> {
    let width = values.len() as u32;
    if width == 0 {
        return None;
    }
    let image_height = height as f32;
    let row_of = |value: f32| image_height / 2. * (1. - (value * gain).clamp(-1., 1.));
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (column_index, &[min, max]) in values.iter().enumerate() {
        if min.is_nan() {
            continue;
        }
        let top = row_of(max);
        let bottom = row_of(min).max(top + 1.).min(image_height);
        let first = top.floor().max(0.) as u32;
        let last = (bottom.ceil() as u32).min(height);
        for row in first..last {
            // How much of this pixel the column covers.
            let cover = ((row + 1) as f32).min(bottom) - (row as f32).max(top);
            let offset = ((row * width + column_index as u32) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&[
                blue,
                green,
                red,
                (alpha as f32 * cover.clamp(0., 1.)).round() as u8,
            ]);
        }
    }
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// `[min, max]` of the lane's channels under column `column_index`: from the samples
/// when zoomed in (the samples either side of a column, so they read as a
/// line past a sample per pixel), from the peak pyramid otherwise.
fn column_values(
    peaks: &WaveformPeaks,
    raw: Option<&SampleBuffer>,
    start: f64,
    samples_per_column: f64,
    channels: &Range<usize>,
    column_index: usize,
) -> [f32; 2] {
    let first_sample = (start + column_index as f64 * samples_per_column)
        .floor()
        .max(0.) as usize;
    let end_sample = if samples_per_column < 1. {
        (start + (column_index + 1) as f64 * samples_per_column).ceil() as usize + 1
    } else {
        ((start + (column_index + 1) as f64 * samples_per_column).floor() as usize)
            .max(first_sample + 1)
    };
    channels
        .clone()
        .filter_map(|channel| match raw {
            Some(raw) if samples_per_column < RAW_MAX_SAMPLES_PER_COLUMN => {
                peaks.range_exact(raw, channel, first_sample, end_sample)
            }
            _ => peaks.range_approx(
                channel,
                first_sample,
                end_sample,
                (samples_per_column / 4.) as usize,
            ),
        })
        .reduce(|first, second| (first.0.min(second.0), first.1.max(second.1)))
        .map_or(EMPTY, |(min, max)| [min, max])
}

/// The waveform of `clip` over `rect` (from the clip's start), pass after
/// pass of its loop, as `paint_clip_waveform` in the egui app.
#[allow(clippy::too_many_arguments)]
pub fn paint_clip_waveform(
    window: &mut Window,
    cache: &mut WaveCache,
    clip: &AudioClip,
    rect: Rect,
    viewport: Rect,
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
    let x_of_beat = |beats: f64| rect.left + beats as f32 * pixels_per_beat;
    let file_width = clip.source_beats(bpm) as f32 * pixels_per_beat;
    let beats = |px: f32| ((px - rect.left) / pixels_per_beat) as f64;
    let in_view = beats(viewport.left).max(0.)..beats(viewport.right());
    for segment in clip.segments(in_view, bpm) {
        let (left, right) = (x_of_beat(segment.start), x_of_beat(segment.end));
        if right < viewport.left || left > viewport.right() {
            continue;
        }
        let file_left = left - (segment.file_start / seconds) as f32 * file_width;
        let file = Rect::new(file_left, rect.top, file_width, rect.height);
        let shown = Rect::new(left, rect.top, right - left, rect.height)
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
        if segment.start > 0. && left >= viewport.left && left <= viewport.right() {
            window.paint_quad(gpui::fill(
                Rect::new(left, rect.top, 1., rect.height).bounds(),
                color.opacity(0.43),
            ));
        }
    }
}
