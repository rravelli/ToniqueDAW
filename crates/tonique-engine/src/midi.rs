//! MIDI messages and a fixed-capacity, allocation-free event list.

/// Hard cap on events a node may emit per block. Extra events are dropped
/// (and counted) rather than growing the list on the RT thread.
pub const MAX_MIDI_EVENTS_PER_BLOCK: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    ControlChange { channel: u8, controller: u8, value: u8 },
    /// -8192..=8191, 0 is centre.
    PitchBend { channel: u8, value: i16 },
    AllNotesOff { channel: u8 },
}

impl MidiMessage {
    /// Parse a raw short message. Running status is not supported.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let status = *bytes.first()?;
        let channel = status & 0x0f;
        let d1 = bytes.get(1).copied().unwrap_or(0) & 0x7f;
        let d2 = bytes.get(2).copied().unwrap_or(0) & 0x7f;
        Some(match status & 0xf0 {
            0x80 => Self::NoteOff { channel, note: d1, velocity: d2 },
            0x90 if d2 == 0 => Self::NoteOff { channel, note: d1, velocity: 0 },
            0x90 => Self::NoteOn { channel, note: d1, velocity: d2 },
            0xb0 if d1 == 123 => Self::AllNotesOff { channel },
            0xb0 => Self::ControlChange { channel, controller: d1, value: d2 },
            0xe0 => Self::PitchBend { channel, value: ((d2 as i16) << 7 | d1 as i16) - 8192 },
            _ => return None,
        })
    }

    pub fn to_bytes(self) -> [u8; 3] {
        match self {
            Self::NoteOn { channel, note, velocity } => [0x90 | channel, note, velocity],
            Self::NoteOff { channel, note, velocity } => [0x80 | channel, note, velocity],
            Self::ControlChange { channel, controller, value } => [0xb0 | channel, controller, value],
            Self::PitchBend { channel, value } => {
                let v = (value + 8192).clamp(0, 16383) as u16;
                [0xe0 | channel, (v & 0x7f) as u8, (v >> 7) as u8]
            }
            Self::AllNotesOff { channel } => [0xb0 | channel, 123, 0],
        }
    }
}

/// A MIDI message at a sample offset within the current block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiEvent {
    pub offset: u32,
    pub message: MidiMessage,
}

/// Event list whose storage is reserved up front; `push` never allocates.
#[derive(Debug)]
pub struct MidiEventList {
    events: Vec<MidiEvent>,
    dropped: usize,
}

impl Default for MidiEventList {
    fn default() -> Self {
        Self::with_capacity(MAX_MIDI_EVENTS_PER_BLOCK)
    }
}

impl MidiEventList {
    /// A list with no capacity: every push is dropped. Never allocates.
    pub const fn empty() -> Self {
        Self { events: Vec::new(), dropped: 0 }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { events: Vec::with_capacity(capacity), dropped: 0 }
    }

    /// Appends an event; returns `false` (and counts a drop) when full.
    pub fn push(&mut self, offset: u32, message: MidiMessage) -> bool {
        if self.events.len() == self.events.capacity() {
            self.dropped += 1;
            return false;
        }
        self.events.push(MidiEvent { offset, message });
        true
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.dropped = 0;
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Events dropped since the last `clear` because the list was full.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn iter(&self) -> std::slice::Iter<'_, MidiEvent> {
        self.events.iter()
    }

    /// Append all of `other`'s events.
    pub fn extend_from(&mut self, other: &MidiEventList) {
        for e in other.iter() {
            self.push(e.offset, e.message);
        }
    }

    /// Stable, allocation-free sort by offset (insertion sort: lists are
    /// short and usually nearly sorted). Stability keeps note-off before
    /// note-on at the same offset when they were pushed in that order.
    pub fn sort(&mut self) {
        let ev = &mut self.events;
        for i in 1..ev.len() {
            let mut j = i;
            while j > 0 && ev[j - 1].offset > ev[j].offset {
                ev.swap(j - 1, j);
                j -= 1;
            }
        }
    }
}

impl<'a> IntoIterator for &'a MidiEventList {
    type Item = &'a MidiEvent;
    type IntoIter = std::slice::Iter<'a, MidiEvent>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Tracks which notes are held so they can be released on stop/seek.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActiveNotes {
    bits: [u128; 16],
}

impl ActiveNotes {
    pub fn observe(&mut self, msg: &MidiMessage) {
        match *msg {
            MidiMessage::NoteOn { channel, note, .. } => self.bits[channel as usize & 15] |= 1 << note,
            MidiMessage::NoteOff { channel, note, .. } => self.bits[channel as usize & 15] &= !(1 << note),
            MidiMessage::AllNotesOff { channel } => self.bits[channel as usize & 15] = 0,
            _ => {}
        }
    }

    pub fn any(&self) -> bool {
        self.bits.iter().any(|b| *b != 0)
    }

    /// Emit note-offs for every held note at `offset` and forget them.
    pub fn release_all(&mut self, out: &mut MidiEventList, offset: u32) {
        for (channel, bits) in self.bits.iter_mut().enumerate() {
            while *bits != 0 {
                let note = bits.trailing_zeros() as u8;
                *bits &= !(1 << note);
                out.push(offset, MidiMessage::NoteOff { channel: channel as u8, note, velocity: 0 });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bytes() {
        for m in [
            MidiMessage::NoteOn { channel: 3, note: 60, velocity: 100 },
            MidiMessage::NoteOff { channel: 0, note: 1, velocity: 0 },
            MidiMessage::ControlChange { channel: 15, controller: 7, value: 127 },
            MidiMessage::PitchBend { channel: 1, value: -8192 },
            MidiMessage::PitchBend { channel: 1, value: 8191 },
            MidiMessage::AllNotesOff { channel: 2 },
        ] {
            assert_eq!(MidiMessage::from_bytes(&m.to_bytes()), Some(m));
        }
    }

    #[test]
    fn list_is_bounded_and_sorts_stably() {
        let mut l = MidiEventList::with_capacity(3);
        let off = MidiMessage::NoteOff { channel: 0, note: 60, velocity: 0 };
        let on = MidiMessage::NoteOn { channel: 0, note: 60, velocity: 1 };
        assert!(l.push(5, on));
        assert!(l.push(2, off));
        assert!(l.push(2, on));
        assert!(!l.push(0, on));
        assert_eq!(l.dropped(), 1);
        l.sort();
        let v: Vec<_> = l.iter().map(|e| (e.offset, e.message)).collect();
        assert_eq!(v, vec![(2, off), (2, on), (5, on)]);
    }

    #[test]
    fn active_notes_release() {
        let mut a = ActiveNotes::default();
        a.observe(&MidiMessage::NoteOn { channel: 1, note: 64, velocity: 9 });
        a.observe(&MidiMessage::NoteOn { channel: 1, note: 127, velocity: 9 });
        let mut out = MidiEventList::default();
        a.release_all(&mut out, 7);
        assert_eq!(out.len(), 2);
        assert!(!a.any());
    }
}
