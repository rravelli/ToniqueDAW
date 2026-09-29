//! Play the demo song on the default output device, and tweak it live:
//! `cargo run --release -p tonique-engine --example play --features device`

mod demo;

use std::time::Duration;

use demo::demo_edit;
use tonique_engine::device::OutputDevice;
use tonique_engine::edit::commands::{SetMute, SetParam};
use tonique_engine::edit::{ChannelRef, EditSession};
use tonique_engine::engine::{Engine, EngineConfig};
use tonique_engine::time::BeatPos;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let device = OutputDevice::default_output()?;
    let config = EngineConfig {
        sample_rate: device.sample_rate(),
        max_block: 256,
        worker_threads: 2,
        ..Default::default()
    };
    let (engine, processor) = Engine::new(config);
    let mut session =
        EditSession::new(demo_edit(config.sample_rate), engine).map_err(|e| e.to_string())?;
    let _stream = device.start(processor)?;

    session
        .set_loop(Some((BeatPos(0.0), BeatPos(16.0))))
        .map_err(|e| e.to_string())?;
    session.play().map_err(|e| e.to_string())?;
    let bass = session.edit().tracks[1].id;
    let arp_vol = session.edit().tracks[0].channel.volume.id;
    for step in 0.. {
        std::thread::sleep(Duration::from_millis(500));
        // Live edits while playing: mute the bass for a while, ride a fader.
        match step % 32 {
            8 => session.perform(SetMute::new(ChannelRef::Track(bass), true)),
            16 => session.perform(SetMute::new(ChannelRef::Track(bass), false)),
            20 => session.perform(SetParam::new(arp_vol, 0.4)),
            28 => session.perform(SetParam::new(arp_vol, 1.0)),
            _ => Ok(()),
        }
        .map_err(|e| e.to_string())?;
        println!(
            "beat {:6.2}  cpu {:4.1}%",
            session.position().0,
            session.engine().cpu_load() * 100.0
        );
    }
    Ok(())
}
