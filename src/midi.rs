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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_copies_short_messages_and_decodes_status_channel() {
        let midi = MidiCopy::from_bytes(&[0x91, 64], 42);

        assert_eq!(midi.len, 2);
        assert_eq!(midi.data, [0x91, 64, 0]);
        assert_eq!(midi.status, 0x9);
        assert_eq!(midi.channel, 0x1);
        assert_eq!(midi.time, 42);
    }

    #[test]
    fn from_bytes_truncates_long_messages_to_fixed_realtime_copy_size() {
        let midi = MidiCopy::from_bytes(&[0xB0, 41, 127, 99, 88], 7);

        assert_eq!(midi.len, 3);
        assert_eq!(midi.data, [0xB0, 41, 127]);
        assert_eq!(midi.status, 0xB);
        assert_eq!(midi.channel, 0);
    }

    #[test]
    fn from_bytes_handles_empty_messages_as_zeroed_data() {
        let midi = MidiCopy::from_bytes(&[], 3);

        assert_eq!(midi.len, 0);
        assert_eq!(midi.data, [0, 0, 0]);
        assert_eq!(midi.status, 0);
        assert_eq!(midi.channel, 0);
    }

    #[test]
    fn debug_output_uses_fixed_safe_slots_even_for_short_messages() {
        let midi = MidiCopy::from_bytes(&[0x80], 5);

        assert_eq!(
            format!("{midi:?}"),
            "Midi { time: 5, len: 1, status_byte: 10000000, status: 1000, channel: 0 data1: 0, data2: 0 }"
        );
    }

    #[test]
    fn unpack_splits_status_nibble_and_channel_nibble() {
        assert_eq!(unpack(0xBF), [0xB, 0xF]);
    }
}
