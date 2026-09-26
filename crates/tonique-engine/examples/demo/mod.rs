//! A small demo song shared by the examples.

use std::sync::Arc;

use tonique_engine::automation::{BeatPoint, CurveShape};
use tonique_engine::edit::{Bus, Clip, Edit, Note, Plugin, PluginKind, Send, Track};
use tonique_engine::nodes::{Envelope, FilterMode};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::BeatPos;

/// Four bars: a synth arpeggio through an automated filter sweep, a bass
/// line, a synthetic percussion sample on an audio track, and an echo bus.
pub fn demo_edit(sample_rate: f64) -> Edit {
    let mut edit = Edit::new(118.0);
    let bus = Bus::new(&mut edit, "Echo");

    // Arpeggio with a filter sweep.
    let mut arp = Track::new(&mut edit, "Arp");
    let synth = Plugin::new(&mut edit, PluginKind::Synth(Envelope { attack_s: 0.002, decay_s: 0.15, sustain: 0.3, release_s: 0.1 }));
    arp.channel.plugins.push(synth);
    let mut filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::LowPass));
    filter.params[0].automation = vec![
        BeatPoint { beat: BeatPos(0.0), value: 300.0, shape: CurveShape::Bezier { c1: 0.1, c2: 0.3 } },
        BeatPoint { beat: BeatPos(12.0), value: 6000.0, shape: CurveShape::Linear },
        BeatPoint { beat: BeatPos(16.0), value: 800.0, shape: CurveShape::Linear },
    ];
    filter.params[1].value.set(3.0, 0.0);
    arp.channel.plugins.push(filter);
    let chord = [57u8, 60, 64, 67, 72, 67, 64, 60];
    let notes = (0..64).map(|i| Note { start: i as f64 * 0.25, length: 0.2, pitch: chord[i % 8] + if i >= 32 { 2 } else { 0 }, velocity: 70 + (i % 4) as u8 * 15 }).collect();
    let clip = Clip::midi(&mut edit, BeatPos(0.0), 16.0, notes);
    arp.clips.push(clip);
    let send = Send::new(&mut edit, bus.id, 0.5);
    arp.sends.push(send);
    arp.channel.pan.value.set(-0.3, 0.0);
    edit.tracks.push(arp);

    // Bass.
    let mut bass = Track::new(&mut edit, "Bass");
    let synth = Plugin::new(&mut edit, PluginKind::Synth(Envelope { attack_s: 0.005, decay_s: 0.3, sustain: 0.7, release_s: 0.08 }));
    bass.channel.plugins.push(synth);
    let roots = [33u8, 33, 36, 31];
    let notes = (0..16).map(|i| Note { start: i as f64, length: 0.8, pitch: roots[i / 4], velocity: 110 }).collect();
    let clip = Clip::midi(&mut edit, BeatPos(0.0), 16.0, notes);
    bass.clips.push(clip);
    bass.channel.volume.value.set(0.8, 0.0);
    edit.tracks.push(bass);

    // Percussion: a synthesized "kick" sample, one clip per beat. All clips
    // share the same Arc'd sample data.
    let kick = edit.add_source(Arc::new(synth_kick(sample_rate)));
    let mut perc = Track::new(&mut edit, "Kick");
    for beat in 0..16 {
        let clip = Clip::audio(&mut edit, BeatPos(beat as f64), 0.9, kick);
        perc.clips.push(clip);
    }
    perc.channel.pan.value.set(0.1, 0.0);
    edit.tracks.push(perc);

    let mut bus = bus;
    let echo = Plugin::new(&mut edit, PluginKind::Echo { time_s: 60.0 / 118.0 * 0.75 });
    echo.params[0].value.set(0.45, 0.0);
    echo.params[1].value.set(1.0, 0.0);
    bus.channel.plugins.push(echo);
    bus.channel.pan.value.set(0.3, 0.0);
    edit.buses.push(bus);
    edit.master.volume.value.set(0.7, 0.0);
    edit
}

fn synth_kick(sr: f64) -> SampleBuffer {
    let len = (sr * 0.35) as usize;
    let mut phase = 0.0f64;
    let data = (0..len)
        .map(|i| {
            let t = i as f64 / sr;
            let freq = 45.0 + 120.0 * (-t * 30.0).exp();
            phase += freq / sr;
            ((phase * std::f64::consts::TAU).sin() * (-t * 9.0).exp() * 0.9) as f32
        })
        .collect();
    SampleBuffer::new(vec![data], sr)
}
