//! Multi-resolution min/max peaks ("waveform thumbnails") for drawing audio.
//!
//! Level 0 stores the min and max of every [`BASE_BUCKET`] samples; each
//! following level merges [`LEVEL_FACTOR`] buckets of the level below. A
//! query over any sample range then touches O(levels × factor) buckets,
//! whatever the file length or zoom. Built off the RT thread, incrementally
//! while a file decodes ([`PeakBuilder`]) or from a whole [`SampleBuffer`].

use crate::sample::SampleBuffer;

/// Samples per bucket at level 0.
pub const BASE_BUCKET: usize = 64;
/// Buckets of level `n` merged into one bucket of level `n + 1`.
pub const LEVEL_FACTOR: usize = 4;

const MAGIC: &[u8; 4] = b"TQPK";
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
struct PeakLevel {
    /// Samples per bucket.
    bucket: usize,
    min: Vec<f32>,
    max: Vec<f32>,
}

/// Immutable peak pyramid, one set of levels per channel.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WaveformPeaks {
    channels: Vec<Vec<PeakLevel>>,
    frames: usize,
    sample_rate: f64,
}

impl WaveformPeaks {
    pub fn from_buffer(buffer: &SampleBuffer) -> Self {
        let mut builder = PeakBuilder::new(buffer.num_channels(), buffer.sample_rate());
        let channels: Vec<&[f32]> = (0..buffer.num_channels()).map(|c| buffer.channel(c)).collect();
        builder.push(&channels);
        builder.snapshot()
    }

    /// Frames covered so far (grows while a file decodes).
    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// `(min, max)` of channel `ch` over the samples `start..end`, or `None`
    /// when the range holds no data yet. Edges are rounded out to level-0
    /// buckets, so the result may include up to `BASE_BUCKET - 1` samples
    /// beyond each end.
    pub fn range(&self, ch: usize, start: usize, end: usize) -> Option<(f32, f32)> {
        let levels = self.channels.get(ch)?;
        let end = end.min(self.frames);
        if start >= end || levels.is_empty() {
            return None;
        }
        let span = end - start;
        let top = levels.iter().rposition(|l| l.bucket <= span).unwrap_or(0);
        let (min, max) = range_at(levels, top, start, end);
        (min <= max).then_some((min, max))
    }

    /// Like [`Self::range`], but reads a single level: the coarsest whose
    /// buckets hold at most `resolution` samples. Edges are rounded out to
    /// that level's buckets. Cheaper than `range` when drawing many
    /// adjacent columns: pass about a quarter of the samples per column.
    pub fn range_approx(&self, ch: usize, start: usize, end: usize, resolution: usize) -> Option<(f32, f32)> {
        let levels = self.channels.get(ch)?;
        let end = end.min(self.frames);
        if start >= end || levels.is_empty() {
            return None;
        }
        let level = &levels[levels.iter().rposition(|l| l.bucket <= resolution).unwrap_or(0)];
        let (min, max) = reduce(level, start / level.bucket, end.div_ceil(level.bucket).min(level.min.len()));
        (min <= max).then_some((min, max))
    }

    /// Compact little-endian encoding, for on-disk caches.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&(self.frames as u64).to_le_bytes());
        out.extend_from_slice(&(self.channels.len() as u32).to_le_bytes());
        for levels in &self.channels {
            out.extend_from_slice(&(levels.len() as u32).to_le_bytes());
            for level in levels {
                out.extend_from_slice(&(level.bucket as u64).to_le_bytes());
                out.extend_from_slice(&(level.min.len() as u64).to_le_bytes());
                level.min.iter().chain(&level.max).for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
            }
        }
        out
    }

    /// Inverse of [`Self::to_bytes`]; `None` on any malformed input.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        if r.take(4)? != MAGIC || r.u32()? != VERSION {
            return None;
        }
        let sample_rate = f64::from_le_bytes(r.take(8)?.try_into().ok()?);
        let frames = r.u64()? as usize;
        let num_channels = r.u32()?;
        let mut channels = Vec::new();
        for _ in 0..num_channels {
            let num_levels = r.u32()?;
            let mut levels = Vec::new();
            for _ in 0..num_levels {
                let bucket = r.u64()? as usize;
                let len = r.u64()? as usize;
                let min = r.f32s(len)?;
                let max = r.f32s(len)?;
                if bucket == 0 {
                    return None;
                }
                levels.push(PeakLevel { bucket, min, max });
            }
            channels.push(levels);
        }
        r.0.is_empty().then_some(Self { channels, frames, sample_rate })
    }
}

/// Min/max over `start..end` (with `end <= frames`) using level `l` for
/// whole buckets and finer levels for the partial edges.
fn range_at(levels: &[PeakLevel], l: usize, start: usize, end: usize) -> (f32, f32) {
    let level = &levels[l];
    let b = level.bucket;
    if l == 0 {
        let last = end.div_ceil(b).min(level.min.len());
        return reduce(level, start / b, last);
    }
    let first_full = start.div_ceil(b);
    let last_full = end / b;
    if first_full >= last_full {
        return range_at(levels, l - 1, start, end);
    }
    let mut acc = reduce(level, first_full, last_full.min(level.min.len()));
    if start < first_full * b {
        acc = merge(acc, range_at(levels, l - 1, start, first_full * b));
    }
    if last_full * b < end {
        acc = merge(acc, range_at(levels, l - 1, last_full * b, end));
    }
    acc
}

fn reduce(level: &PeakLevel, from: usize, to: usize) -> (f32, f32) {
    let from = from.min(to);
    let min = level.min[from..to].iter().copied().fold(f32::INFINITY, f32::min);
    let max = level.max[from..to].iter().copied().fold(f32::NEG_INFINITY, f32::max);
    (min, max)
}

fn merge(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0.min(b.0), a.1.max(b.1))
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32s(&mut self, n: usize) -> Option<Vec<f32>> {
        let bytes = self.take(n.checked_mul(4)?)?;
        Some(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }
}

/// Builds a [`WaveformPeaks`] from planar blocks as they are decoded.
pub struct PeakBuilder {
    channels: Vec<ChannelBuilder>,
    frames: usize,
    sample_rate: f64,
}

#[derive(Default)]
struct ChannelBuilder {
    /// Completed buckets per level.
    levels: Vec<PeakLevel>,
    /// Bucket being filled per level: (min, max, items merged so far).
    pending: Vec<(f32, f32, usize)>,
}

impl ChannelBuilder {
    fn push_sample(&mut self, s: f32) {
        self.push_at(0, s, s);
    }

    fn push_at(&mut self, l: usize, min: f32, max: f32) {
        if l == self.levels.len() {
            let bucket = BASE_BUCKET * LEVEL_FACTOR.pow(l as u32);
            self.levels.push(PeakLevel { bucket, min: Vec::new(), max: Vec::new() });
            self.pending.push((f32::INFINITY, f32::NEG_INFINITY, 0));
        }
        let per_bucket = if l == 0 { BASE_BUCKET } else { LEVEL_FACTOR };
        let p = &mut self.pending[l];
        p.0 = p.0.min(min);
        p.1 = p.1.max(max);
        p.2 += 1;
        if p.2 == per_bucket {
            let (min, max, _) = std::mem::replace(p, (f32::INFINITY, f32::NEG_INFINITY, 0));
            self.levels[l].min.push(min);
            self.levels[l].max.push(max);
            self.push_at(l + 1, min, max);
        }
    }

    /// Completed levels plus the partially filled trailing buckets.
    fn snapshot(&self) -> Vec<PeakLevel> {
        let mut levels = self.levels.clone();
        // A pending bucket also covers the pending data of every level below.
        let mut carry: Option<(f32, f32)> = None;
        for (level, &(min, max, count)) in levels.iter_mut().zip(&self.pending) {
            let tail = match (count > 0, carry) {
                (true, Some(c)) => Some(merge((min, max), c)),
                (true, None) => Some((min, max)),
                (false, c) => c,
            };
            if let Some((min, max)) = tail {
                level.min.push(min);
                level.max.push(max);
            }
            carry = tail;
        }
        levels
    }
}

impl PeakBuilder {
    pub fn new(num_channels: usize, sample_rate: f64) -> Self {
        Self { channels: (0..num_channels).map(|_| ChannelBuilder::default()).collect(), frames: 0, sample_rate }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Append one planar block; all channels must have the same length.
    pub fn push(&mut self, block: &[&[f32]]) {
        assert_eq!(block.len(), self.channels.len(), "channel count mismatch");
        let len = block.first().map_or(0, |c| c.len());
        assert!(block.iter().all(|c| c.len() == len), "channels must have equal length");
        for (builder, samples) in self.channels.iter_mut().zip(block) {
            samples.iter().for_each(|&s| builder.push_sample(s));
        }
        self.frames += len;
    }

    /// Peaks for everything pushed so far, including partial buckets.
    pub fn snapshot(&self) -> WaveformPeaks {
        WaveformPeaks { channels: self.channels.iter().map(ChannelBuilder::snapshot).collect(), frames: self.frames, sample_rate: self.sample_rate }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random samples in [-1, 1).
    fn noise(len: usize, seed: u64) -> Vec<f32> {
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 40) as f32 / (1u64 << 23) as f32 - 1.0
            })
            .collect()
    }

    fn brute(samples: &[f32], start: usize, end: usize) -> (f32, f32) {
        let s = &samples[start..end.min(samples.len())];
        (s.iter().copied().fold(f32::INFINITY, f32::min), s.iter().copied().fold(f32::NEG_INFINITY, f32::max))
    }

    #[test]
    fn range_matches_brute_force_on_bucket_rounded_edges() {
        let samples = noise(100_003, 1);
        let peaks = WaveformPeaks::from_buffer(&SampleBuffer::new(vec![samples.clone()], 48000.));
        let spans: [(usize, usize); 8] = [(0, 1), (5, 70), (63, 65), (0, 100_003), (1000, 90_000), (12_345, 67_890), (99_990, 200_000), (4096, 8192)];
        for (start, end) in spans {
            let rounded = (start / BASE_BUCKET * BASE_BUCKET, end.div_ceil(BASE_BUCKET) * BASE_BUCKET);
            assert_eq!(peaks.range(0, start, end), Some(brute(&samples, rounded.0, rounded.1)), "{start}..{end}");
        }
    }

    #[test]
    fn approx_range_rounds_to_the_chosen_level() {
        let samples = noise(100_000, 8);
        let peaks = WaveformPeaks::from_buffer(&SampleBuffer::new(vec![samples.clone()], 48000.));
        let spans: [(usize, usize, usize); 5] = [(10, 20, 1), (100, 900, 200), (1_000, 9_000, 2_000), (5, 99_999, 30_000), (70_000, 200_000, 4_096)];
        for (start, end, resolution) in spans {
            let bucket = [BASE_BUCKET, 256, 1024, 4096, 16384].into_iter().rfind(|&b| b <= resolution).unwrap_or(BASE_BUCKET);
            let rounded = (start / bucket * bucket, end.div_ceil(bucket) * bucket);
            assert_eq!(peaks.range_approx(0, start, end, resolution), Some(brute(&samples, rounded.0, rounded.1)), "{start}..{end}");
        }
        assert_eq!(peaks.range_approx(0, 100_000, 100_010, 64), None);
    }

    #[test]
    fn aligned_ranges_are_exact() {
        let samples = noise(50_000, 2);
        let peaks = WaveformPeaks::from_buffer(&SampleBuffer::new(vec![samples.clone()], 48000.));
        for start in (0..40_000).step_by(BASE_BUCKET * 37) {
            for len in [BASE_BUCKET, 256, 1024, 3200, 9984] {
                assert_eq!(peaks.range(0, start, start + len), Some(brute(&samples, start, start + len)));
            }
        }
    }

    #[test]
    fn incremental_build_equals_one_shot() {
        let (l, r) = (noise(70_001, 3), noise(70_001, 4));
        let one_shot = WaveformPeaks::from_buffer(&SampleBuffer::new(vec![l.clone(), r.clone()], 44100.));
        let mut builder = PeakBuilder::new(2, 44100.);
        let mut at = 0;
        for chunk in [1, 63, 1000, 4097, 20_000, 44_840] {
            builder.push(&[&l[at..at + chunk], &r[at..at + chunk]]);
            at += chunk;
        }
        assert_eq!(builder.snapshot(), one_shot);
    }

    #[test]
    fn partial_snapshot_covers_pushed_data() {
        let samples = noise(10_000, 5);
        let mut builder = PeakBuilder::new(1, 48000.);
        builder.push(&[&samples[..3_000]]);
        let peaks = builder.snapshot();
        assert_eq!(peaks.frames(), 3_000);
        assert_eq!(peaks.range(0, 0, 3_000), Some(brute(&samples, 0, 3_000)));
        assert_eq!(peaks.range(0, 3_000, 5_000), None);
    }

    #[test]
    fn many_channels_are_independent() {
        let chans: Vec<Vec<f32>> = (0..5).map(|c| noise(5_000, 10 + c)).collect();
        let peaks = WaveformPeaks::from_buffer(&SampleBuffer::new(chans.clone(), 48000.));
        assert_eq!(peaks.num_channels(), 5);
        for (c, samples) in chans.iter().enumerate() {
            assert_eq!(peaks.range(c, 0, 5_000), Some(brute(samples, 0, 5_000)));
        }
        assert_eq!(peaks.range(5, 0, 10), None);
    }

    #[test]
    fn bytes_round_trip() {
        let peaks = WaveformPeaks::from_buffer(&SampleBuffer::new(vec![noise(33_333, 6), noise(33_333, 7)], 48000.));
        let bytes = peaks.to_bytes();
        assert_eq!(WaveformPeaks::from_bytes(&bytes), Some(peaks));
        assert_eq!(WaveformPeaks::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(WaveformPeaks::from_bytes(b"nope"), None);
    }

    #[test]
    fn empty_peaks() {
        let peaks = WaveformPeaks::default();
        assert_eq!(peaks.range(0, 0, 100), None);
        assert_eq!(PeakBuilder::new(2, 48000.).snapshot().range(0, 0, 100), None);
    }
}
