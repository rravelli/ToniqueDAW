use crate::{
    audio::engine::Engine,
    core::{export::ExportStatus, message::ProcessToGuiMsg},
};
use crossbeam::channel::Sender;
use hound::{SampleFormat, WavSpec, WavWriter};

const EXPORT_CHUNK: usize = 1024;

pub fn export_audio(
    mut engine: Engine,
    total_frames: usize,
    path: std::path::PathBuf,
    sender: Sender<ProcessToGuiMsg>,
) {
    let _ = sender.send(ProcessToGuiMsg::ExportUpdate(ExportStatus::PROCESSING(0.)));
    let spec = WavSpec {
        channels: 2,
        sample_rate: engine.sample_rate as u32,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut writer = match WavWriter::create(&path, spec) {
        Ok(w) => w,
        Err(err) => {
            let _ = sender.send(ProcessToGuiMsg::ExportUpdate(ExportStatus::FAILED(
                err.to_string(),
            )));
            return;
        }
    };

    let mut pos = 0;
    let mut metrics = crate::core::metrics::GlobalMetrics::new();

    while pos < total_frames {
        engine.process(pos, EXPORT_CHUNK, &mut metrics);
        if let Some(master) = engine.tracks.get("master") {
            for sample in master.mix.iter() {
                writer.write_sample(*sample).unwrap();
            }
        }

        pos += EXPORT_CHUNK;
        let _ = sender.send(ProcessToGuiMsg::ExportUpdate(ExportStatus::PROCESSING(
            pos as f32 / total_frames as f32,
        )));
    }
    writer.finalize().unwrap();
    let _ = sender.send(ProcessToGuiMsg::ExportUpdate(ExportStatus::DONE));
}
