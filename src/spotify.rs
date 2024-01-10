use rustbus::{DuplexConn, get_session_bus_path, MessageBuilder};
use rustbus::connection::Timeout;
use rustbus::message_builder::MarshalledMessage;
use std::sync::mpsc::SyncSender;
use crate::midi;

const SPOTIFY_DST: &str = "org.mpris.MediaPlayer2.spotify";
const SPOTIFY_PATH: &str = "/org/mpris/MediaPlayer2";

pub struct Spotify {
    connection: DuplexConn,
    sender: SyncSender<midi::MidiCopy>
}

impl Spotify {
    pub fn new(sender: SyncSender<midi::MidiCopy>) -> Result<Spotify, rustbus::connection::Error> {
        let mut spotify = Spotify {
            connection: DuplexConn::connect_to_bus(get_session_bus_path()?, true)?,
            sender
        };

        spotify.connection.send_hello(Timeout::Infinite)?;

        Ok(spotify)
    }

    pub fn handle_midi(&mut self, m: midi::MidiCopy, action: &str) -> Result<(), rustbus::connection::Error> {
        if action == "pass-through" {
            self.sender.try_send(m).unwrap_or_else(|err| eprintln!("Error sending midi message: {}", err));
        } else {
            let call = self.call(action);
            self.connection.send.send_message(&call)?.write_all().unwrap();
        }

        Ok(())
    }

    fn call(&self, method: &str) -> MarshalledMessage {
        MessageBuilder::new()
            .call(method)
            .with_interface("org.mpris.MediaPlayer2.Player")
            .on(SPOTIFY_PATH)
            .at(SPOTIFY_DST)
            .build()
    }
}
