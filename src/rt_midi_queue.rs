use crate::midi;

pub struct Producer {
    inner: rtrb::Producer<midi::MidiCopy>,
}

pub struct Consumer {
    inner: rtrb::Consumer<midi::MidiCopy>,
}

pub fn channel(capacity: usize) -> (Producer, Consumer) {
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    (Producer { inner: producer }, Consumer { inner: consumer })
}

impl Producer {
    pub fn try_push(&mut self, midi: midi::MidiCopy) -> Result<(), midi::MidiCopy> {
        self.inner.push(midi).map_err(|err| match err {
            rtrb::PushError::Full(midi) => midi,
        })
    }
}

impl Consumer {
    pub fn try_pop(&mut self) -> Option<midi::MidiCopy> {
        self.inner.pop().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_starts_empty() {
        let (_producer, mut consumer) = channel(1);

        assert_eq!(consumer.try_pop(), None);
    }

    #[test]
    fn queue_preserves_fifo_order() {
        let (mut producer, mut consumer) = channel(2);
        let first = midi::MidiCopy::from_bytes(&[0x90, 60, 127], 1);
        let second = midi::MidiCopy::from_bytes(&[0x80, 60, 0], 2);

        producer.try_push(first).unwrap();
        producer.try_push(second).unwrap();

        assert_eq!(consumer.try_pop(), Some(first));
        assert_eq!(consumer.try_pop(), Some(second));
        assert_eq!(consumer.try_pop(), None);
    }

    #[test]
    fn full_queue_returns_unwritten_midi_event() {
        let (mut producer, mut consumer) = channel(1);
        let queued = midi::MidiCopy::from_bytes(&[0xB0, 41, 127], 3);
        let rejected = midi::MidiCopy::from_bytes(&[0xB0, 42, 127], 4);

        producer.try_push(queued).unwrap();

        assert_eq!(producer.try_push(rejected), Err(rejected));
        assert_eq!(consumer.try_pop(), Some(queued));
        assert_eq!(consumer.try_pop(), None);
    }
}
