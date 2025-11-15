use crate::audio::track::TrackBackend;

#[derive(Clone, Debug)]
pub struct BusTrackData {
    pub children: Vec<String>,
}
impl BusTrackData {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }
}

impl BusTrackData {
    pub fn process(
        &mut self,
        mix: &mut Vec<f32>,
        children: Vec<TrackBackend>,
        solo_tracks: &Vec<String>,
    ) {
        for track in children {
            if !track.disabled(solo_tracks) {
                for i in 0..mix.len() {
                    mix[i] += track.mix[i];
                }
            }
        }
    }
}
