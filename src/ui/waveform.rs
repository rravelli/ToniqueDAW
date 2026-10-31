use std::{
    collections::HashMap,
    hash::Hash,
    ops::Range,
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
/// `[min, max]` of a column with no audio under it.
const EMPTY: [f32; 2] = [f32::NAN; 2];

/// Draws the samples `samples` (fractional frame positions) of `data`
/// stretched over `rect`, painting only the part inside `visible`.
///
/// To show part of the audio (a trimmed clip), pass the rect of the whole
/// audio and restrict `visible`, so trimming doesn't move the columns.
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
    samples: Range<f64>,
    stereo: bool,
    color: Color32,
    gain: f32,
    pan: f32,
) {
    let peaks = data.peaks();
    let num_channels = peaks.num_channels();
    if num_channels == 0
        || rect.width() <= 0.
        || samples.end <= samples.start
        || !rect.intersects(visible)
    {
        return;
    }
    // Whole tiles are drawn: kept to `visible`, they don't spill over
    // what's beside it (another pass of a clip's loop, say).
    let painter = &painter.with_clip_rect(visible);
    let columns = Columns::new(rect, visible, &samples, painter.pixels_per_point() as f64);
    let lanes = lanes(rect, stereo, num_channels, gain, pan);
    let raw = data.samples().map(|s| s.as_ref());

    // Zoomed in past one sample per pixel: join the samples with a line.
    if columns.spp < 1.
        && let Some(raw) = raw
    {
        paint_samples_line(
            painter,
            &columns,
            &lanes,
            visible,
            raw,
            samples.start,
            color,
        );
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
    let zoom = ZoomKey {
        peaks: Arc::as_ptr(&peaks) as usize,
        raw: raw.is_some(),
        start: samples.start.to_bits(),
        spp: columns.spp.to_bits(),
        stereo,
    };

    // Nothing moved since last frame (e.g. during playback): reuse the mesh.
    let mesh_key = MeshKey {
        zoom,
        rect: rect_bits(rect),
        visible: rect_bits(visible),
        ppp: columns.ppp.to_bits(),
        color,
        gain: gain.to_bits(),
        pan: pan.to_bits(),
    };
    if let Some(mesh) = cache.mesh(&mesh_key) {
        painter.add(Shape::Mesh(mesh));
        return;
    }

    let source = Source {
        peaks: &peaks,
        raw,
        start: samples.start,
        spp: columns.spp,
    };
    // Tiles are meshed in tile-local coordinates, so scrolling only copies
    // and shifts them (and x stays precise far from the audio's start).
    let mut mesh = Mesh::default();
    for (l, lane) in lanes.iter().enumerate() {
        if !lane.is_visible(visible) {
            continue;
        }
        let local_lane = Lane {
            top: lane.top - rect.top(),
            ..lane.clone()
        };
        for tile in columns.tiles() {
            let tile_key = TileKey { zoom, tile };
            let tile_mesh_key = TileMeshKey {
                tile: tile_key,
                lane: l,
                lane_rect: [local_lane.top, local_lane.height].map(f32::to_bits),
                ppp: columns.ppp.to_bits(),
                color,
                gain: local_lane.gain.to_bits(),
            };
            let tile_mesh = match cache.tile_mesh(&tile_mesh_key) {
                Some(tile_mesh) => tile_mesh,
                None => {
                    let values = cache.values(tile_key, || Tile {
                        _peaks: peaks.clone(),
                        values: source.tile(&lanes, columns.tile(tile)),
                        lanes: lanes.len(),
                    });
                    let tile_mesh =
                        Arc::new(mesh_tile(values.lane(l), &local_lane, columns.ppp, color));
                    cache.insert_tile_mesh(tile_mesh_key, tile_mesh.clone());
                    tile_mesh
                }
            };
            let n = mesh.vertices.len();
            mesh.append_ref(&tile_mesh);
            let offset = vec2(columns.to_x((tile * TILE) as f64), rect.top());
            for v in &mut mesh.vertices[n..] {
                v.pos += offset;
            }
        }
    }
    let mesh = Arc::new(mesh);
    cache.insert_mesh(mesh_key, mesh.clone());
    painter.add(Shape::Mesh(mesh));
}

/// The physical pixel columns the audio is drawn in.
///
/// Columns are anchored to `rect`'s left edge (snapped to the pixel grid),
/// so the samples behind each column only depend on the zoom and never on
/// the scroll position: no jitter.
struct Columns {
    ppp: f64,
    /// Physical x of the first column.
    origin: f64,
    /// Samples per column.
    spp: f64,
    /// Columns at least partly inside `visible`.
    visible: Range<usize>,
    count: usize,
}

impl Columns {
    fn new(rect: Rect, visible: Rect, samples: &Range<f64>, ppp: f64) -> Self {
        let origin = (rect.left() as f64 * ppp).round();
        let count = (rect.width() as f64 * ppp).ceil() as usize;
        let column = |x: f32| (x as f64 * ppp - origin).clamp(0., count as f64);
        Self {
            ppp,
            origin,
            spp: (samples.end - samples.start) / (rect.width() as f64 * ppp),
            visible: column(visible.left()).floor() as usize
                ..column(visible.right()).ceil() as usize,
            count,
        }
    }

    /// Screen x of the left edge of (fractional) column `px`.
    fn to_x(&self, px: f64) -> f32 {
        ((self.origin + px) / self.ppp) as f32
    }

    /// Tiles holding visible columns.
    fn tiles(&self) -> Range<usize> {
        self.visible.start / TILE..self.visible.end.div_ceil(TILE)
    }

    /// Columns of `tile`.
    fn tile(&self, tile: usize) -> Range<usize> {
        tile * TILE..((tile + 1) * TILE).min(self.count)
    }
}

#[derive(Clone)]
struct Lane {
    channels: Range<usize>,
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

    fn is_visible(&self, visible: Rect) -> bool {
        self.top < visible.bottom() && self.top + self.height > visible.top()
    }
}

/// One lane per channel with `stereo`, else one for all.
fn lanes(rect: Rect, stereo: bool, num_channels: usize, gain: f32, pan: f32) -> Vec<Lane> {
    let (left, right) = balance(pan);
    if !stereo {
        return vec![Lane {
            channels: 0..num_channels,
            top: rect.top(),
            height: rect.height(),
            gain: gain * (left + right) / 2.,
        }];
    }
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
}

/// Joins the visible samples with a line, one per lane.
fn paint_samples_line(
    painter: &Painter,
    columns: &Columns,
    lanes: &[Lane],
    visible: Rect,
    raw: &SampleBuffer,
    sample_start: f64,
    color: Color32,
) {
    let spp = columns.spp;
    let stroke = Stroke::new(1., color);
    let from = (sample_start + columns.visible.start as f64 * spp)
        .floor()
        .max(0.) as usize;
    let to = ((sample_start + columns.visible.end as f64 * spp).ceil() as usize + 1).min(raw.len());
    for lane in lanes.iter().filter(|l| l.is_visible(visible)) {
        let points: Vec<Pos2> = (from..to)
            .map(|i| {
                let v = lane
                    .channels
                    .clone()
                    .map(|ch| raw.channel(ch)[i])
                    .sum::<f32>()
                    / lane.channels.len() as f32;
                pos2(columns.to_x((i as f64 - sample_start) / spp), lane.y(v))
            })
            .collect();
        painter.add(Shape::line(points, stroke));
    }
}

/// Mesh of one lane of a tile, in tile-local coordinates.
fn mesh_tile(values: &[[f32; 2]], lane: &Lane, ppp: f64, color: Color32) -> Mesh {
    let min_height = 1. / ppp as f32;
    let mut mesh = Mesh::default();
    let mut strip = Strip::new(&mut mesh, color);
    for (i, column) in values.iter().copied().enumerate() {
        if is_empty(column) {
            strip.end();
            continue;
        }
        let [min, max] = column;
        let px = i as f64;
        let top = lane.y(max);
        let bottom = lane.y(min).max(top + min_height);
        strip.column((px / ppp) as f32, ((px + 1.) / ppp) as f32, top, bottom);
    }
    strip.end();
    mesh
}

fn is_empty(column: [f32; 2]) -> bool {
    column[0].is_nan()
}

struct Source<'a> {
    peaks: &'a WaveformPeaks,
    raw: Option<&'a SampleBuffer>,
    start: f64,
    spp: f64,
}

impl Source<'_> {
    /// `[min, max]` of each of `columns`, lane after lane.
    fn tile(&self, lanes: &[Lane], columns: Range<usize>) -> Vec<[f32; 2]> {
        lanes
            .iter()
            .flat_map(|lane| columns.clone().map(|px| self.column(lane, px)))
            .collect()
    }

    /// `[min, max]` of the lane's channels under column `px`, [`EMPTY`] if
    /// none.
    fn column(&self, lane: &Lane, px: usize) -> [f32; 2] {
        let start = (self.start + px as f64 * self.spp).floor().max(0.) as usize;
        let end = ((self.start + (px + 1) as f64 * self.spp).floor() as usize).max(start + 1);
        lane.channels
            .clone()
            .filter_map(|ch| self.range(ch, start, end))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
            .map_or(EMPTY, |(min, max)| [min, max])
    }

    /// Exact from the samples when a column spans few of them, from the
    /// peak pyramid otherwise (rounded to buckets of a quarter column).
    fn range(&self, ch: usize, start: usize, end: usize) -> Option<(f32, f32)> {
        match self.raw {
            Some(raw) if self.spp < RAW_MAX_SPP => self.peaks.range_exact(raw, ch, start, end),
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

fn rect_bits(rect: Rect) -> [u32; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(f32::to_bits)
}

/// What column values depend on: the audio, the zoom and the trim.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ZoomKey {
    /// Identity of the peak snapshot (kept alive by the tiles).
    peaks: usize,
    raw: bool,
    /// `f64` bits.
    start: u64,
    /// `f64` bits.
    spp: u64,
    stereo: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TileKey {
    zoom: ZoomKey,
    tile: usize,
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MeshKey {
    zoom: ZoomKey,
    rect: [u32; 4],
    visible: [u32; 4],
    ppp: u64,
    color: Color32,
    /// `f32` bits.
    gain: u32,
    /// `f32` bits.
    pan: u32,
}

struct Tile {
    _peaks: Arc<WaveformPeaks>,
    /// `[min, max]` per column, lane after lane.
    values: Vec<[f32; 2]>,
    lanes: usize,
}

impl Tile {
    fn lane(&self, lane: usize) -> &[[f32; 2]] {
        let len = self.values.len() / self.lanes;
        &self.values[lane * len..(lane + 1) * len]
    }
}

/// Column values of recently drawn tiles, and recently drawn meshes, so
/// frames where only the position changes (scrolling, playback) don't
/// touch the audio data.
#[derive(Default)]
struct TileCache {
    frame: u64,
    tiles: FrameCache<TileKey, Tile>,
    tile_meshes: FrameCache<TileMeshKey, Arc<Mesh>>,
    meshes: FrameCache<MeshKey, Arc<Mesh>>,
}

impl TileCache {
    fn start_frame(&mut self, frame: u64) {
        if frame != self.frame {
            self.frame = frame;
            self.tiles.evict_unused(frame);
            self.tile_meshes.evict_unused(frame);
            self.meshes.evict_unused(frame);
        }
    }

    fn mesh(&mut self, key: &MeshKey) -> Option<Arc<Mesh>> {
        self.meshes.get(key, self.frame).cloned()
    }

    fn insert_mesh(&mut self, key: MeshKey, mesh: Arc<Mesh>) {
        self.meshes.insert(key, mesh, self.frame);
    }

    fn tile_mesh(&mut self, key: &TileMeshKey) -> Option<Arc<Mesh>> {
        self.tile_meshes.get(key, self.frame).cloned()
    }

    fn insert_tile_mesh(&mut self, key: TileMeshKey, mesh: Arc<Mesh>) {
        self.tile_meshes.insert(key, mesh, self.frame);
    }

    fn values(&mut self, key: TileKey, compute: impl FnOnce() -> Tile) -> &Tile {
        self.tiles.get_or_insert_with(key, self.frame, compute)
    }
}

/// Entries tagged with the frame they were last used in; those not used
/// during the previous frame are dropped.
struct FrameCache<K, V>(HashMap<K, (V, u64)>);

impl<K, V> Default for FrameCache<K, V> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<K: Eq + Hash, V> FrameCache<K, V> {
    fn evict_unused(&mut self, frame: u64) {
        self.0.retain(|_, (_, last_used)| *last_used + 1 >= frame);
    }

    fn get(&mut self, key: &K, frame: u64) -> Option<&V> {
        let (value, last_used) = self.0.get_mut(key)?;
        *last_used = frame;
        Some(value)
    }

    fn insert(&mut self, key: K, value: V, frame: u64) {
        self.0.insert(key, (value, frame));
    }

    fn get_or_insert_with(&mut self, key: K, frame: u64, compute: impl FnOnce() -> V) -> &V {
        let (value, last_used) = self.0.entry(key).or_insert_with(|| (compute(), frame));
        *last_used = frame;
        value
    }
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
