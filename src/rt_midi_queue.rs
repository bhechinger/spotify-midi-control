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
