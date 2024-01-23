use crate::MAX_MIDI;

#[derive(Copy, Clone)]
pub struct MidiCopy {
    pub len: usize,
    pub data: [u8; MAX_MIDI],
    pub status: u8,
    pub channel: u8,
    pub time: jack::Frames,
}

impl From<jack::RawMidi<'_>> for MidiCopy {
    fn from(midi: jack::RawMidi<'_>) -> Self {
        let len = std::cmp::min(MAX_MIDI, midi.bytes.len());
        let mut data = [0; MAX_MIDI];
        data[..len].copy_from_slice(&midi.bytes[..len]);
        let status = unpack(data[0]);
        MidiCopy {
            len,
            data,
            status: status[0],
            channel: status[1],
            time: midi.time,
        }
    }
}

impl std::fmt::Debug for MidiCopy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Midi {{ time: {}, len: {}, status_byte: {:b}, status: {:b}, channel: {:b} data1: {}, data2: {} }}",
            self.time,
            self.len,
            self.data[0],
            self.status,
            self.channel,
            self.data[1],
            self.data[2]
        )
    }
}

fn unpack(val: u8) -> [u8; 2] {
    [val >> 4, val & 0b1111]
}
