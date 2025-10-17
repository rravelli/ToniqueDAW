use midir::{Ignore, MidiInput, MidiOutput};
use midly::live::LiveEvent;
use rtrb::Producer;
use std::thread;

pub fn spawn_midi_thread(mut tx: Producer<Vec<u8>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut midi_in = MidiInput::new("midi-input").expect("Failed to create MidiInput");
        let midi_out = MidiOutput::new("Hello").expect("Failed to create MidiOutput");

        midi_in.ignore(Ignore::None);

        let port = &midi_out.ports()[0];
        let mut conn_out = midi_out
            .connect(port, "Midi out")
            .expect("Failed to connect");

        let port = &midi_in.ports()[1];

        let _conn_in = midi_in
            .connect(
                port,
                "midir-read-input",
                move |_, message, _| {
                    let res = LiveEvent::parse(message);
                    match res {
                        Ok(event) => match event {
                            LiveEvent::Midi {
                                channel: _,
                                message: _,
                            } => todo!(),
                            LiveEvent::Common(_) => todo!(),
                            LiveEvent::Realtime(_) => todo!(),
                        },
                        Err(_) => {}
                    };
                    if message.len() >= 3 {
                        let _ = tx.push(message.to_vec());
                        let _ = conn_out.send(message);
                    }
                },
                (),
            )
            .expect("Failed to connect to MIDI input port");
        // Keep the thread alive to receive events
        loop {
            thread::park();
        }
    })
}
