use crate::MAX_MIDI;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct MidiCopy {
    pub len: usize,
    pub data: [u8; MAX_MIDI],
    pub status: u8,
    pub channel: u8,
    pub time: u32,
}

impl MidiCopy {
    pub fn from_bytes(bytes: &[u8], time: u32) -> Self {
        let len = std::cmp::min(MAX_MIDI, bytes.len());
        let mut data = [0; MAX_MIDI];
        data[..len].copy_from_slice(&bytes[..len]);
        let status = unpack(data[0]);
        MidiCopy {
            len,
            data,
            status: status[0],
            channel: status[1],
            time,
        }
    }
}

impl From<jack::RawMidi<'_>> for MidiCopy {
    fn from(midi: jack::RawMidi<'_>) -> Self {
        MidiCopy::from_bytes(midi.bytes, midi.time)
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
