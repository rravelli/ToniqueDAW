//! The built-in effects on known signals, through the whole engine.

use std::f32::consts::PI;
use std::sync::Arc;

use tonique_engine::dsp::{Oversampler, db_to_gain, gain_to_db};
use tonique_engine::edit::commands::*;
use tonique_engine::edit::{Clip, Edit, EditSession, Plugin, PluginKind, Track};
use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig, render_offline};
use tonique_engine::nodes::{EQ_BANDS, FilterMode, SaturationCurve, eq_mode, eq_mode_value};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::BeatPos;

const SR: f64 = 48000.0;
const BPM: f64 = 120.0;

fn engine() -> (Engine, AudioProcessor) {
    Engine::new(EngineConfig {
        sample_rate: SR,
        max_block: 256,
        output_channels: 2,
        worker_threads: 0,
        parallel_threshold: 4,
        housekeeping_thread: false,
    })
}

fn sine(hz: f32, amplitude: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (2.0 * PI * hz * i as f32 / SR as f32).sin() * amplitude)
        .collect()
}

fn impulse(len: usize) -> Vec<f32> {
    let mut data = vec![0.0; len];
    data[0] = 1.0;
    data
}

/// `input` on both sides through one plugin, set up by `setup`.
struct Rig {
    session: EditSession,
    processor: AudioProcessor,
    plugin: Plugin,
}

impl Rig {
    fn new(kind: PluginKind, input: Vec<f32>, setup: &[(&str, f32)]) -> Self {
        let mut edit = Edit::new(BPM);
        let beats = input.len() as f64 / SR * BPM / 60.0;
        let src = edit.add_source(Arc::new(SampleBuffer::new(vec![input.clone(), input], SR)));
        let mut track = Track::new(&mut edit, "t");
        let mut clip = Clip::audio(&mut edit, BeatPos(0.0), beats, src);
        clip.fade_in_s = 0.0;
        clip.fade_out_s = 0.0;
        track.clips.push(clip);
        let plugin = Plugin::new(&mut edit, kind);
        for (name, value) in setup {
            plugin.param(name).expect("param").value.set(*value, 0.0);
        }
        track.channel.plugins.push(plugin.clone());
        edit.tracks.push(track);
        let (e, processor) = engine();
        let mut session = EditSession::new(edit, e).expect("session");
        session.play().expect("play");
        Self {
            session,
            processor,
            plugin,
        }
    }

    /// The next `frames` of the left side.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        render_offline(&mut self.processor, frames, 2)
            .into_iter()
            .step_by(2)
            .collect()
    }
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(buf: &[f32]) -> f32 {
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

#[test]
fn compressor_reduces_by_the_ratio_over_the_threshold() {
    // -6 dBFS peaks, 12 dB over a -18 dB threshold: 4:1 takes 9 dB off.
    let mut rig = Rig::new(
        PluginKind::Compressor,
        sine(400.0, 0.5, 48000),
        &[
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("attack", 1.0),
        ],
    );
    rig.render(24000);
    let level = gain_to_db(peak(&rig.render(4800)));
    assert!((level - -15.0).abs() < 0.5, "{level} dB");
    let reduction = rig.plugin.tap.reduction.take().expect("measured");
    assert!((reduction - 9.0).abs() < 0.5, "{reduction} dB");

    // Makeup gain puts it back up.
    rig.plugin.param("makeup").unwrap().set(9.0);
    rig.render(4800);
    let level = gain_to_db(peak(&rig.render(4800)));
    assert!((level - -6.0).abs() < 0.5, "{level} dB");
}

#[test]
fn compressor_leaves_audio_under_the_threshold_alone() {
    let input = sine(400.0, 0.05, 24000);
    let mut rig = Rig::new(
        PluginKind::Compressor,
        input.clone(),
        &[("threshold", -12.0), ("knee", 0.0)],
    );
    let out = rig.render(24000);
    assert!(out.iter().zip(&input).all(|(a, b)| (a - b).abs() < 1e-6));
}

#[test]
fn limiter_never_goes_over_its_ceiling() {
    // A loud mix of sines, made louder, with a sudden peak.
    let len = 48000;
    let mut input: Vec<f32> = (0..len)
        .map(|i| {
            let t = i as f32 / SR as f32;
            0.6 * (2.0 * PI * 110.0 * t).sin()
                + 0.4 * (2.0 * PI * 1730.0 * t).sin()
                + 0.3 * (2.0 * PI * 7100.0 * t).sin()
        })
        .collect();
    input[30_000] = 4.0;
    let ceiling = -3.0;
    let mut rig = Rig::new(
        PluginKind::Limiter { lookahead_ms: 5.0 },
        input,
        &[("gain", 12.0), ("ceiling", ceiling), ("release", 20.0)],
    );
    let out = rig.render(len);
    let most = peak(&out);
    assert!(
        most <= db_to_gain(ceiling) + 1e-6,
        "{} dB",
        gain_to_db(most)
    );
    assert!(gain_to_db(most) > ceiling - 1.0, "and it gets there");
    assert!(rig.plugin.tap.reduction.take().expect("measured") > 10.0);
}

#[test]
fn limiter_reports_its_lookahead_as_latency() {
    let lookahead_ms = 2.0;
    let samples = (lookahead_ms * 0.001 * SR) as usize;
    let mut rig = Rig::new(
        PluginKind::Limiter {
            lookahead_ms: lookahead_ms as f32,
        },
        // Under the ceiling: untouched, just late.
        impulse(4800).into_iter().map(|s| s * 0.5).collect(),
        &[],
    );
    let stats = rig.session.last_compile_stats().unwrap();
    assert_eq!(stats.output_latency, samples);
    let out = rig.render(480);
    // The metronome's path is delayed to match; the clip comes out where
    // it went in, the rest of the graph lined up behind it.
    assert_eq!(out.iter().filter(|s| s.abs() > 1e-6).count(), 1);
    assert!((peak(&out) - 0.5).abs() < 1e-6);
}

#[test]
fn eq_is_neutral_until_a_band_is_moved() {
    let input = sine(1000.0, 0.5, 48000);
    let mut rig = Rig::new(PluginKind::Eq, input.clone(), &[]);
    let out = rig.render(24000);
    let worst = out
        .iter()
        .zip(&input)
        .fold(0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(worst < 1e-4, "{worst}");

    // A bell, switched on and raised 12 dB right on the sine.
    let [freq, gain, _, shape, on] = EQ_BANDS[3].params;
    assert_eq!(
        eq_mode(rig.plugin.param(shape).unwrap().get()),
        FilterMode::Peak
    );
    rig.plugin.param(freq).unwrap().set(1000.0);
    rig.plugin.param(gain).unwrap().set(12.0);
    rig.plugin.param(on).unwrap().set(1.0);
    rig.render(4800);
    let level = gain_to_db(peak(&rig.render(4800)));
    assert!((level - (-6.02 + 12.0)).abs() < 0.2, "{level} dB");
}

#[test]
fn eq_cuts_switch_in_and_out() {
    let low = sine(50.0, 0.5, 96000);
    let mut rig = Rig::new(PluginKind::Eq, low, &[("b1_freq", 500.0)]);
    rig.render(9600);
    assert!(peak(&rig.render(4800)) > 0.49, "off at first");
    rig.plugin.param("b1_on").unwrap().set(1.0);
    rig.render(9600);
    assert!(peak(&rig.render(4800)) < 0.01, "cuts the lows once on");
}

#[test]
fn eq_bands_take_any_shape_without_clicks() {
    // A bell at 500 Hz, flat, turned into a low cut while a low sine plays.
    let low = sine(50.0, 0.5, 96000);
    let mut rig = Rig::new(PluginKind::Eq, low, &[("b4_freq", 500.0), ("b4_on", 1.0)]);
    rig.render(9600);
    assert!(
        peak(&rig.render(4800)) > 0.49,
        "a flat bell changes nothing"
    );
    rig.plugin
        .param("b4_type")
        .unwrap()
        .set(eq_mode_value(FilterMode::HighPass));
    let switch = rig.render(4800);
    let jump = switch
        .windows(2)
        .fold(0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(jump < 0.02, "clicked: {jump}");
    rig.render(9600);
    assert!(peak(&rig.render(4800)) < 0.01, "now it cuts the lows");
}

#[test]
fn reverb_decays_by_60_db_over_its_decay_time() {
    let decay = 1.0;
    let mut rig = Rig::new(
        PluginKind::Reverb,
        impulse(4 * 48000),
        &[("decay", decay), ("mix", 1.0), ("predelay", 0.0)],
    );
    let out = rig.render(3 * 48000);
    assert!(out.iter().all(|s| s.is_finite()));
    let window = |from_s: f32| {
        let start = (from_s * SR as f32) as usize;
        rms(&out[start..start + 4800])
    };
    let fall = gain_to_db(window(0.2)) - gain_to_db(window(0.2 + decay));
    assert!((fall - 60.0).abs() < 10.0, "fell {fall} dB in {decay} s");
    assert!(window(0.05) > 1e-3, "it's heard");
}

#[test]
fn reverb_pre_delay_holds_it_back() {
    let mut rig = Rig::new(
        PluginKind::Reverb,
        impulse(48000),
        &[("predelay", 100.0), ("mix", 1.0)],
    );
    let out = rig.render(9600);
    let first = out.iter().position(|s| s.abs() > 1e-6).unwrap();
    assert!(first >= 4800, "first heard at {first}");
}

#[test]
fn reverb_tails_survive_edits() {
    // As an echo's do: rebuilding mid-tail changes nothing.
    let make = || {
        Rig::new(
            PluginKind::Reverb,
            impulse(48000),
            &[("decay", 3.0), ("mix", 1.0)],
        )
    };
    let (mut edited, mut reference) = (make(), make());
    let (mut a, mut b) = (edited.render(12000), reference.render(12000));
    let empty = edited.session.create(|e| Track::new(e, "empty"));
    edited.session.perform(AddTrack::new(empty)).unwrap();
    a.extend(edited.render(12000));
    b.extend(reference.render(12000));
    assert!(peak(&a[12000..]) > 1e-3);
    assert_eq!(a, b);
}

#[test]
fn saturation_bounds_peaks_and_reports_its_latency() {
    let mut rig = Rig::new(
        PluginKind::Saturation(SaturationCurve::HardClip),
        sine(200.0, 0.5, 48000),
        &[("drive", 24.0)],
    );
    let stats = rig.session.last_compile_stats().unwrap();
    assert_eq!(stats.output_latency, Oversampler::LATENCY);
    rig.render(4800);
    let out = rig.render(9600);
    // Flat tops at full scale, a little ripple from the oversampler.
    let most = peak(&out);
    assert!(most > 0.95 && most < 1.15, "{most}");
    assert!(rms(&out) > 0.85, "squared off: {}", rms(&out));
}

#[test]
fn saturation_mix_blends_the_dry_signal_in_step() {
    let input = sine(300.0, 0.5, 24000);
    let mut rig = Rig::new(
        PluginKind::Saturation(SaturationCurve::SoftClip),
        input.clone(),
        &[("mix", 0.0), ("drive", 24.0)],
    );
    let out = rig.render(24000);
    // All dry: the input, as late as the wet would be.
    let lag = Oversampler::LATENCY;
    let worst = out[lag..]
        .iter()
        .zip(&input)
        .fold(0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(worst < 1e-6, "{worst}");
}

#[test]
fn gate_silences_quiet_audio_and_lets_loud_audio_through() {
    let len = 48000;
    // Loud for the first half, then a quiet hum.
    let mut input = sine(300.0, 0.5, len);
    for s in &mut input[len / 2..] {
        *s *= 0.001;
    }
    let mut rig = Rig::new(
        PluginKind::Gate,
        input,
        &[("threshold", -30.0), ("hold", 10.0), ("release", 20.0)],
    );
    let out = rig.render(len);
    assert!((peak(&out[4800..len / 2]) - 0.5).abs() < 1e-3, "open");
    assert!(peak(&out[len / 2 + 9600..]) < 1e-6, "closed");
    // A gentler range only turns it down.
    let mut rig = Rig::new(
        PluginKind::Gate,
        sine(300.0, 0.001, len),
        &[("threshold", -30.0), ("range", -20.0), ("release", 20.0)],
    );
    let level = gain_to_db(peak(&rig.render(len)[24000..]));
    assert!((level - (-60.0 - 20.0)).abs() < 0.5, "{level} dB");
}

#[test]
fn synced_echoes_repeat_on_the_beat() {
    // Half a beat at 120 BPM: a quarter of a second.
    let mut rig = Rig::new(
        PluginKind::Echo {
            time_s: 0.1,
            sync: Some(0.5),
        },
        impulse(48000),
        &[("mix", 1.0)],
    );
    let out = rig.render(24000);
    let heard: Vec<usize> = (1..out.len()).filter(|&i| out[i].abs() > 0.01).collect();
    assert_eq!(heard.first(), Some(&12000));
}

#[test]
fn echo_repeats_are_filtered() {
    // A low sine through an echo whose low cut is far above it: the
    // repeats are mostly gone, the dry signal not.
    let mut rig = Rig::new(
        PluginKind::Echo {
            time_s: 0.05,
            sync: None,
        },
        sine(60.0, 0.5, 96000),
        &[("mix", 1.0), ("feedback", 0.5), ("low_cut", 2000.0)],
    );
    rig.render(24000);
    let level = peak(&rig.render(4800));
    assert!((level - 0.5).abs() < 0.03, "{level}");
}

#[test]
fn filter_shelves_and_peaks_use_their_gain() {
    let mut rig = Rig::new(
        PluginKind::Filter(FilterMode::Peak),
        sine(1000.0, 0.25, 48000),
        &[("cutoff", 1000.0), ("gain", 6.0206)],
    );
    rig.render(9600);
    assert!((peak(&rig.render(4800)) - 0.5).abs() < 0.005);
}
