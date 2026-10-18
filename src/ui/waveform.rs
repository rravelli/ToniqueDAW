use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use egui::{Color32, Id, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use tonique_engine::{
    nodes::balance,
    peaks::{BASE_BUCKET, WaveformPeaks},
    sample::SampleBuffer,
};

use crate::analysis::AudioData;

/// Columns per cached tile.
const TILE: usize = 256;
/// Below this many samples per column, read the samples themselves.
const RAW_MAX_SPP: f64 = (4 * BASE_BUCKET) as f64;

/// Draws the samples `samples` (fractional frame positions) of `data`
/// stretched over `rect`, painting only the part inside `visible`.
///
/// Columns are one physical pixel wide and anchored to `rect`'s left edge
/// (snapped to the pixel grid), so the samples behind each column only
/// depend on the zoom and never on the scroll position: no jitter. To show
/// part of the audio (a trimmed clip), pass the rect of the whole audio and
/// restrict `visible`, so trimming doesn't move the columns either. Column
/// values are cached in tiles, so frames where only the position changes
/// (scrolling, playback) don't touch the audio data.
/// With `stereo`, each channel gets its own lane; otherwise channels are
/// merged into one. Sample values are drawn multiplied by `gain` (linear)
/// and by the side's share of the `pan` balance (left lane, right lane; a
/// merged lane gets their mean), so the waveform shows what's heard; past
/// full scale, they're cut at the lane's edges.
#[allow(clippy::too_many_arguments)]
pub fn paint_waveform(
    painter: &Painter,
    rect: Rect,
    visible: Rect,
    data: &AudioData,
    samples: std::ops::Range<f64>,
    stereo: bool,
    color: Color32,
    gain: f32,
    pan: f32,
) {
    let (sample_start, sample_end) = (samples.start, samples.end);
    let peaks = data.peaks();
    let num_channels = peaks.num_channels();
    if num_channels == 0
        || rect.width() <= 0.
        || sample_end <= sample_start
        || !rect.intersects(visible)
    {
        return;
    }
    // Whole tiles are drawn: kept to `visible`, they don't spill over
    // what's beside it (another pass of a clip's loop, say).
    let painter = &painter.with_clip_rect(visible);
    let ppp = painter.pixels_per_point() as f64;
    let origin = (rect.left() as f64 * ppp).round();
    let columns = (rect.width() as f64 * ppp).ceil() as usize;
    let spp = (sample_end - sample_start) / (rect.width() as f64 * ppp);
    let first = (visible.left() as f64 * ppp - origin)
        .floor()
        .clamp(0., columns as f64) as usize;
    let last = (visible.right() as f64 * ppp - origin)
        .ceil()
        .clamp(0., columns as f64) as usize;

    let (left, right) = balance(pan);
    let lanes: Vec<Lane> = if stereo {
        let height = rect.height() / num_channels as f32;
        (0..num_channels)
            .map(|ch| Lane {
                channels: ch..ch + 1,
                top: rect.top() + ch as f32 * height,
                height,
                gain: gain
                    * match ch {
                        0 => left,
                        1 => right,
                        _ => 1.,
                    },
            })
            .collect()
    } else {
        vec![Lane {
            channels: 0..num_channels,
            top: rect.top(),
            height: rect.height(),
            gain: gain * (left + right) / 2.,
        }]
    };
    let raw = data.samples().map(|s| s.as_ref());
    let to_x = |px: f64| ((origin + px) / ppp) as f32;
    let lane_visible =
        |lane: &Lane| lane.top < visible.bottom() && lane.top + lane.height > visible.top();

    // Zoomed in past one sample per pixel: join the samples with a line.
    if spp < 1.
        && let Some(raw) = raw
    {
        let stroke = Stroke::new(1., color);
        let from = (sample_start + first as f64 * spp).floor().max(0.) as usize;
        let to = ((sample_start + last as f64 * spp).ceil() as usize + 1).min(raw.len());
        for lane in lanes.iter().filter(|l| lane_visible(l)) {
            let points: Vec<Pos2> = (from..to)
                .map(|i| {
                    let v = lane
                        .channels
                        .clone()
                        .map(|ch| raw.channel(ch)[i])
                        .sum::<f32>()
                        / lane.channels.len() as f32;
                    pos2(to_x((i as f64 - sample_start) / spp), lane.y(v))
                })
                .collect();
            painter.add(Shape::line(points, stroke));
        }
        return;
    }

    let cache = painter.ctx().data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<TileCache>>>(Id::new("waveform_tiles"))
            .clone()
    });
    let Ok(mut cache) = cache.lock() else {
        return;
    };
    cache.start_frame(painter.ctx().cumulative_frame_nr());
    let source = Source {
        peaks: &peaks,
        raw,
        start: sample_start,
        spp,
    };
    let key = |tile| TileKey {
        peaks: Arc::as_ptr(&peaks) as usize,
        raw: raw.is_some(),
        start: sample_start.to_bits(),
        spp: spp.to_bits(),
        stereo,
        tile,
    };

    // Nothing moved since last frame (e.g. during playback): reuse the mesh.
    let mesh_key = MeshKey {
        tiles: key(0),
        rect: [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(f32::to_bits),
        visible: [visible.min.x, visible.min.y, visible.max.x, visible.max.y].map(f32::to_bits),
        ppp: ppp.to_bits(),
        color,
        gain: gain.to_bits(),
        pan: pan.to_bits(),
    };
    if let Some(mesh) = cache.mesh(&mesh_key) {
        painter.add(Shape::Mesh(mesh));
        return;
    }

    // Tiles are meshed in tile-local coordinates, so scrolling only copies
    // and shifts them (and x stays precise far from the audio's start).
    let min_height = 1. / ppp as f32;
    let mut mesh = Mesh::default();
    for (l, lane) in lanes.iter().enumerate() {
        if !lane_visible(lane) {
            continue;
        }
        let local_lane = Lane {
            channels: lane.channels.clone(),
            top: lane.top - rect.top(),
            height: lane.height,
            gain: lane.gain,
        };
        for tile in first / TILE..last.div_ceil(TILE) {
            let tile_mesh_key = TileMeshKey {
                tile: key(tile),
                lane: l,
                lane_rect: [local_lane.top, local_lane.height].map(f32::to_bits),
                ppp: ppp.to_bits(),
                color,
                gain: local_lane.gain.to_bits(),
            };
            let tile_mesh = match cache.tile_mesh(&tile_mesh_key) {
                Some(tile_mesh) => tile_mesh,
                None => {
                    let values = cache.values(key(tile), &peaks, || {
                        let cols = tile * TILE..((tile + 1) * TILE).min(columns);
                        lanes
                            .iter()
                            .flat_map(|lane| cols.clone().map(|px| source.column(lane, px)))
                            .collect()
                    });
                    let tile_len = values.len() / lanes.len();
                    let lane_values = &values[l * tile_len..(l + 1) * tile_len];
                    let mut tile_mesh = Mesh::default();
                    let mut strip = Strip::new(&mut tile_mesh, color);
                    for (i, [min, max]) in lane_values.iter().copied().enumerate() {
                        if min.is_nan() {
                            strip.end();
                            continue;
                        }
                        let px = i as f64;
                        let top = local_lane.y(max);
                        let bottom = local_lane.y(min).max(top + min_height);
                        strip.column((px / ppp) as f32, ((px + 1.) / ppp) as f32, top, bottom);
                    }
                    strip.end();
                    let tile_mesh = Arc::new(tile_mesh);
                    cache.insert_tile_mesh(tile_mesh_key, tile_mesh.clone());
                    tile_mesh
                }
            };
            let n = mesh.vertices.len();
            mesh.append_ref(&tile_mesh);
            let offset = vec2(to_x((tile * TILE) as f64), rect.top());
            for v in &mut mesh.vertices[n..] {
                v.pos += offset;
            }
        }
    }
    let mesh = Arc::new(mesh);
    cache.insert_mesh(mesh_key, mesh.clone());
    painter.add(Shape::Mesh(mesh));
}

struct Lane {
    channels: std::ops::Range<usize>,
    top: f32,
    height: f32,
    /// Sample values are drawn multiplied by it.
    gain: f32,
}

impl Lane {
    /// Screen y of a sample value (times the gain), positive values going
    /// up, cut at full scale.
    fn y(&self, v: f32) -> f32 {
        self.top + self.height / 2. * (1. - (v * self.gain).clamp(-1., 1.))
    }
}

struct Source<'a> {
    peaks: &'a WaveformPeaks,
    raw: Option<&'a SampleBuffer>,
    start: f64,
    spp: f64,
}

impl Source<'_> {
    /// `[min, max]` of the lane's channels under column `px`, NaN if none.
    fn column(&self, lane: &Lane, px: usize) -> [f32; 2] {
        let start = (self.start + px as f64 * self.spp).floor().max(0.) as usize;
        let end = ((self.start + (px + 1) as f64 * self.spp).floor() as usize).max(start + 1);
        lane.channels
            .clone()
            .filter_map(|ch| self.range(ch, start, end))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
            .map_or([f32::NAN; 2], |(min, max)| [min, max])
    }

    /// Exact from the samples when a column spans few of them, from the
    /// peak pyramid otherwise (rounded to buckets of a quarter column).
    fn range(&self, ch: usize, start: usize, end: usize) -> Option<(f32, f32)> {
        match self.raw {
            Some(raw) if self.spp < RAW_MAX_SPP => {
                let samples = raw.channel(ch).get(start..end.min(raw.len()))?;
                let min = samples.iter().copied().reduce(f32::min)?;
                let max = samples.iter().copied().reduce(f32::max)?;
                Some((min, max))
            }
            _ => self
                .peaks
                .range_approx(ch, start, end, (self.spp / 4.) as usize),
        }
    }
}

/// Filled band through the columns' centres: two vertices per column, plus
/// one pair at each end of a run so it spans whole columns.
struct Strip<'a> {
    mesh: &'a mut Mesh,
    color: Color32,
    /// Right edge and span of the previous column while in a run.
    last: Option<(f32, f32, f32)>,
}

impl<'a> Strip<'a> {
    fn new(mesh: &'a mut Mesh, color: Color32) -> Self {
        Self {
            mesh,
            color,
            last: None,
        }
    }

    fn column(&mut self, x0: f32, x1: f32, top: f32, bottom: f32) {
        if self.last.is_none() {
            self.pair(x0, top, bottom, false);
        }
        self.pair((x0 + x1) / 2., top, bottom, true);
        self.last = Some((x1, top, bottom));
    }

    fn end(&mut self) {
        if let Some((x, top, bottom)) = self.last.take() {
            self.pair(x, top, bottom, true);
        }
    }

    fn pair(&mut self, x: f32, top: f32, bottom: f32, connect: bool) {
        let n = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(pos2(x, top), self.color);
        self.mesh.colored_vertex(pos2(x, bottom), self.color);
        if connect {
            self.mesh.add_triangle(n - 2, n - 1, n);
            self.mesh.add_triangle(n - 1, n, n + 1);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TileKey {
    /// Identity of the peak snapshot (kept alive by the tile).
    peaks: usize,
    raw: bool,
    start: u64,
    spp: u64,
    stereo: bool,
    tile: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MeshKey {
    /// Tile key of the first tile: identifies data, zoom and trim.
    tiles: TileKey,
    rect: [u32; 4],
    visible: [u32; 4],
    ppp: u64,
    color: Color32,
    /// `f32` bits.
    gain: u32,
    /// `f32` bits.
    pan: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TileMeshKey {
    tile: TileKey,
    lane: usize,
    /// Lane top (relative to the clip) and height.
    lane_rect: [u32; 2],
    ppp: u64,
    color: Color32,
    /// The lane's gain (with its side's pan), `f32` bits.
    gain: u32,
}

struct Tile {
    _peaks: Arc<WaveformPeaks>,
    /// `[min, max]` per column, lane after lane.
    values: Vec<[f32; 2]>,
    last_used: u64,
}

/// Column values of recently drawn tiles, and recently drawn meshes.
/// Entries not used during the previous frame are dropped.
#[derive(Default)]
struct TileCache {
    frame: u64,
    tiles: HashMap<TileKey, Tile>,
    tile_meshes: HashMap<TileMeshKey, (Arc<Mesh>, u64)>,
    meshes: HashMap<MeshKey, (Arc<Mesh>, u64)>,
}

impl TileCache {
    fn start_frame(&mut self, frame: u64) {
        if frame != self.frame {
            self.frame = frame;
            self.tiles.retain(|_, t| t.last_used + 1 >= frame);
            self.tile_meshes
                .retain(|_, (_, last_used)| *last_used + 1 >= frame);
            self.meshes
                .retain(|_, (_, last_used)| *last_used + 1 >= frame);
        }
    }

    fn mesh(&mut self, key: &MeshKey) -> Option<Arc<Mesh>> {
        touch(&mut self.meshes, key, self.frame)
    }

    fn insert_mesh(&mut self, key: MeshKey, mesh: Arc<Mesh>) {
        self.meshes.insert(key, (mesh, self.frame));
    }

    fn tile_mesh(&mut self, key: &TileMeshKey) -> Option<Arc<Mesh>> {
        touch(&mut self.tile_meshes, key, self.frame)
    }

    fn insert_tile_mesh(&mut self, key: TileMeshKey, mesh: Arc<Mesh>) {
        self.tile_meshes.insert(key, (mesh, self.frame));
    }

    fn values(
        &mut self,
        key: TileKey,
        peaks: &Arc<WaveformPeaks>,
        compute: impl FnOnce() -> Vec<[f32; 2]>,
    ) -> &[[f32; 2]] {
        let tile = self.tiles.entry(key).or_insert_with(|| Tile {
            _peaks: peaks.clone(),
            values: compute(),
            last_used: 0,
        });
        tile.last_used = self.frame;
        &tile.values
    }
}

fn touch<K: Eq + std::hash::Hash>(
    map: &mut HashMap<K, (Arc<Mesh>, u64)>,
    key: &K,
    frame: u64,
) -> Option<Arc<Mesh>> {
    let (mesh, last_used) = map.get_mut(key)?;
    *last_used = frame;
    Some(mesh.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_scales_the_waveform_and_full_scale_cuts_it() {
        let lane = |gain| Lane {
            channels: 0..1,
            top: 0.,
            height: 100.,
            gain,
        };
        // Half a full-scale sample: a quarter of the lane above the middle.
        assert_eq!(lane(1.).y(0.5), 25.);
        assert_eq!(lane(0.5).y(0.5), 37.5, "half as loud, half as tall");
        assert_eq!(lane(0.).y(0.5), 50., "silent: flat");
        // Louder than full scale: cut at the lane's edges.
        assert_eq!(lane(4.).y(0.5), 0.);
        assert_eq!(lane(4.).y(-0.5), 100.);
    }

    /// Only the visible part is drawn, though whole tiles are meshed: one
    /// pass of a looped clip doesn't spill over the next.
    #[test]
    fn nothing_is_drawn_outside_the_visible_part() {
        let audio = crate::analysis::AudioInfo::silent(1.);
        let ctx = egui::Context::default();
        // A few samples a column: meshed, in tiles wider than shown.
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(100., 100.));
        let visible = Rect::from_x_y_ranges(30.0..=35., 0.0..=100.);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            paint_waveform(
                ui.painter(),
                rect,
                visible,
                &audio.data,
                0.0..480.,
                true,
                Color32::WHITE,
                1.,
                0.,
            );
        });
        output.textures_delta.clear();
        let meshes: Vec<_> = output
            .shapes
            .iter()
            .filter(|s| matches!(s.shape, Shape::Mesh(_)))
            .collect();
        assert!(!meshes.is_empty());
        for shape in meshes {
            assert!(
                visible.contains_rect(shape.clip_rect),
                "{:?}",
                shape.clip_rect
            );
        }
    }
}
