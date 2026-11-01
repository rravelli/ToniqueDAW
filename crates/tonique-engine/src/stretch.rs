//! Real-time time stretching: playing audio faster or slower while keeping
//! (or moving on its own) its pitch, as it plays.
//!
//! What stretches is a [`Stretcher`], fed a clip's source as it plays; the
//! clip node handles where in the source, latency and jumps (see
//! [`crate::nodes::AudioClipNode`]). [`new_stretcher`] gives the one built
//! in: Signalsmith Stretch, with feature `signalsmith`; [`Stretchers`]
//! hands them over from graph to graph. How a clip is stretched is its
//! [`StretchSettings`].

use std::collections::HashMap;

/// How a stretched clip is stretched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StretchSettings {
    pub window: StretchWindow,
    /// Transposed, keep its formants (a voice's character) where they
    /// are, rather than moving them with the pitch.
    pub formants: bool,
}

/// How much audio a stretcher analyses at once: longer is smoother on
/// sustained, tonal sound, shorter keeps attacks sharper and costs more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StretchWindow {
    Short,
    #[default]
    Normal,
    Long,
}

impl StretchWindow {
    pub const ALL: [Self; 3] = [Self::Short, Self::Normal, Self::Long];

    /// Its length in seconds; the stretcher steps a quarter of it at a
    /// time.
    pub fn seconds(self) -> f64 {
        match self {
            Self::Short => 0.06,
            Self::Normal => 0.12,
            Self::Long => 0.2,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Stretches stereo audio as it's fed: given input at one rate, gives
/// output at another (their ratio being the speed), transposed. Built off
/// the audio thread; its other methods run on it and must not allocate.
///
/// Its *processing time* is where in the input the output is: input is
/// fed [`Self::input_latency`] frames ahead of it, and output comes
/// [`Self::output_latency`] frames behind it.
pub trait Stretcher: Send {
    fn input_latency(&self) -> usize;
    fn output_latency(&self) -> usize;
    /// Transpose by `semitones` (fractions are cents), keeping the
    /// formants where they are with `formants`.
    fn set_transpose(&mut self, semitones: f32, formants: bool);
    /// Forget everything fed, and start again from `input` (interleaved
    /// stereo, [`Self::input_latency`] frames: the processing time is then
    /// at its first frame), about to be played at `speed`.
    fn seek(&mut self, input: &[f32], speed: f64);
    /// Feed `input` and get `output` (both interleaved stereo): as many
    /// input frames over output frames as the speed played at.
    fn process(&mut self, input: &[f32], output: &mut [f32]);
}

/// The stretcher built in for `sample_rate` and `window`, if any:
/// Signalsmith Stretch with feature `signalsmith`. Without one, stretched
/// clips play resampled (their pitch moving with their speed).
pub fn new_stretcher(sample_rate: f64, window: StretchWindow) -> Option<Box<dyn Stretcher>> {
    #[cfg(feature = "signalsmith")]
    {
        Some(Box::new(Signalsmith::new(sample_rate, window)))
    }
    #[cfg(not(feature = "signalsmith"))]
    {
        let _ = (sample_rate, window);
        None
    }
}

/// Stretchers for the successive graphs of an engine. Building one takes
/// about a millisecond, so a clip stretched in the graph before gets a
/// stand-in, its stretcher being handed over to it when the graph is
/// adopted (see [`crate::graph::Node::take_state_from`]); only clips newly
/// stretched, or with another window, get a new one.
#[derive(Default)]
pub struct Stretchers {
    sample_rate: f64,
    /// Input and output latencies, the same for all of a window.
    latency: [Option<(usize, usize)>; 3],
    /// Clips (by ID) stretched in the last graph published, and their
    /// windows.
    held: HashMap<u64, StretchWindow>,
    /// Clips given one in the graph being built.
    built: HashMap<u64, StretchWindow>,
}

impl Stretchers {
    /// Start a graph for `sample_rate`. With `handover`, clips stretched in
    /// the last graph published get their stretcher from it; without, the
    /// last graph won't play (another replaces it, or there's none) and
    /// every clip gets a new one.
    pub fn begin(&mut self, sample_rate: f64, handover: bool) {
        if !handover || sample_rate != self.sample_rate {
            self.held.clear();
        }
        if sample_rate != self.sample_rate {
            self.latency = [None; 3];
        }
        self.sample_rate = sample_rate;
        self.built.clear();
    }

    /// The stretcher for `clip` in the graph being built, analysing
    /// `window` at once: new or a stand-in. None if there's none built in.
    pub fn get(&mut self, clip: u64, window: StretchWindow) -> Option<Box<dyn Stretcher>> {
        self.built.insert(clip, window);
        if self.held.get(&clip) == Some(&window)
            && let Some((input_latency, output_latency)) = self.latency[window.index()]
        {
            return Some(Box::new(Handover {
                input_latency,
                output_latency,
            }));
        }
        let stretcher = new_stretcher(self.sample_rate, window)?;
        self.latency[window.index()] =
            Some((stretcher.input_latency(), stretcher.output_latency()));
        Some(stretcher)
    }

    /// The graph built was published: its clips' stretchers are handed
    /// over to the next.
    pub fn published(&mut self) {
        self.held = std::mem::take(&mut self.built);
    }
}

/// Stands in for a stretcher handed over from the graph before: as late,
/// silent.
struct Handover {
    input_latency: usize,
    output_latency: usize,
}

impl Stretcher for Handover {
    fn input_latency(&self) -> usize {
        self.input_latency
    }

    fn output_latency(&self) -> usize {
        self.output_latency
    }

    fn set_transpose(&mut self, _: f32, _: bool) {}

    fn seek(&mut self, _: &[f32], _: f64) {}

    fn process(&mut self, _: &[f32], output: &mut [f32]) {
        output.fill(0.0);
    }
}

/// Signalsmith Stretch: good between about 0.75× and 1.5×, and over
/// transpositions of several octaves.
#[cfg(feature = "signalsmith")]
pub struct Signalsmith(signalsmith_stretch::Stretch);

#[cfg(feature = "signalsmith")]
impl Signalsmith {
    pub fn new(sample_rate: f64, window: StretchWindow) -> Self {
        let block = (sample_rate * window.seconds()).round() as usize;
        Self(signalsmith_stretch::Stretch::new(2, block, block / 4))
    }
}

#[cfg(feature = "signalsmith")]
impl Stretcher for Signalsmith {
    fn input_latency(&self) -> usize {
        self.0.input_latency()
    }

    fn output_latency(&self) -> usize {
        self.0.output_latency()
    }

    fn set_transpose(&mut self, semitones: f32, formants: bool) {
        self.0.set_transpose_factor_semitones(semitones, None);
        // Only costs anything transposed.
        self.0.set_formant_factor(1.0, formants);
    }

    fn seek(&mut self, input: &[f32], speed: f64) {
        self.0.reset();
        self.0.seek(input, speed);
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        self.0.process(input, output);
    }
}
