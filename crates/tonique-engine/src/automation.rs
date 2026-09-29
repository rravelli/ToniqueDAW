//! Immutable automation curves, evaluated sample-accurately on the RT thread.

use crate::time::{BeatPos, SamplePos, TempoMap};

/// Shape of the segment that starts at a point and runs to the next one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurveShape {
    Step,
    Linear,
    /// 1-D cubic Bézier through (0, c1, c2, 1), mapped onto the segment.
    Bezier {
        c1: f32,
        c2: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub time: SamplePos,
    pub value: f32,
    pub shape: CurveShape,
}

/// Sorted list of points. Edits create a new curve (behind an `Arc`) that is
/// swapped in; a curve is never mutated while the RT thread can see it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutomationCurve {
    points: Vec<CurvePoint>,
}

/// A point expressed in musical time, as stored in the edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatPoint {
    pub beat: BeatPos,
    pub value: f32,
    pub shape: CurveShape,
}

impl AutomationCurve {
    pub fn new(mut points: Vec<CurvePoint>) -> Self {
        points.sort_by_key(|p| p.time);
        Self { points }
    }

    /// Map musical-time points to samples through the tempo map.
    pub fn from_beats(points: &[BeatPoint], tempo: &TempoMap, sample_rate: f64) -> Self {
        Self::new(
            points
                .iter()
                .map(|p| CurvePoint {
                    time: tempo.beats_to_samples(p.beat, sample_rate),
                    value: p.value,
                    shape: p.shape,
                })
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    /// O(log n), allocation-free.
    pub fn value_at(&self, pos: SamplePos) -> f32 {
        let idx = self.points.partition_point(|p| p.time <= pos);
        match (idx.checked_sub(1), self.points.get(idx)) {
            (Some(a), Some(b)) => interpolate(&self.points[a], b, pos),
            (Some(a), None) => self.points[a].value,
            (None, _) => self.points.first().map_or(0.0, |p| p.value),
        }
    }
}

fn interpolate(a: &CurvePoint, b: &CurvePoint, pos: SamplePos) -> f32 {
    let span = (b.time - a.time) as f64;
    if span <= 0.0 {
        return b.value;
    }
    let t = ((pos - a.time) as f64 / span).clamp(0.0, 1.0) as f32;
    let shaped = match a.shape {
        CurveShape::Step => 0.0,
        CurveShape::Linear => t,
        CurveShape::Bezier { c1, c2 } => {
            let u = 1.0 - t;
            3.0 * u * u * t * c1 + 3.0 * u * t * t * c2 + t * t * t
        }
    };
    a.value + (b.value - a.value) * shaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(time: SamplePos, value: f32, shape: CurveShape) -> CurvePoint {
        CurvePoint { time, value, shape }
    }

    #[test]
    fn evaluates_shapes() {
        let c = AutomationCurve::new(vec![
            pt(100, 0.0, CurveShape::Linear),
            pt(200, 1.0, CurveShape::Step),
            pt(300, 0.0, CurveShape::Bezier { c1: 0.0, c2: 1.0 }),
            pt(400, 1.0, CurveShape::Linear),
        ]);
        assert_eq!(c.value_at(0), 0.0); // before first: hold first
        assert_eq!(c.value_at(150), 0.5);
        assert_eq!(c.value_at(250), 1.0); // step holds
        assert_eq!(c.value_at(300), 0.0);
        assert_eq!(c.value_at(350), 0.5); // symmetric bezier midpoint
        assert_eq!(c.value_at(10_000), 1.0);
        assert_eq!(AutomationCurve::default().value_at(5), 0.0);
    }

    #[test]
    fn follows_tempo_map() {
        let mut t = TempoMap::new(120.0);
        t.set_tempo(BeatPos(2.0), 60.0);
        let c = AutomationCurve::from_beats(
            &[BeatPoint {
                beat: BeatPos(3.0),
                value: 1.0,
                shape: CurveShape::Linear,
            }],
            &t,
            1000.0,
        );
        // beat 2 @120bpm = 1 s, one more beat @60 = 1 s -> 2000 samples at 1 kHz
        assert_eq!(c.points()[0].time, 2000);
    }
}
