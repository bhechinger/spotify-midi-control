use rustbus::connection::Timeout;
use rustbus::message_builder::MarshalledMessage;
use rustbus::{get_session_bus_path, DuplexConn, MessageBuilder};
use std::time::Duration;

const SPOTIFY_DST: &str = "org.mpris.MediaPlayer2.spotify";
const SPOTIFY_PATH: &str = "/org/mpris/MediaPlayer2";
const DBUS_TIMEOUT: Timeout = Timeout::Duration(Duration::from_secs(2));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Play,
    Pause,
    Previous,
    Next,
}

impl Action {
    fn mpris_method(self) -> &'static str {
        match self {
            Action::Play => "Play",
            Action::Pause => "Pause",
            Action::Previous => "Previous",
            Action::Next => "Next",
        }
    }
}

pub struct Spotify {
    connection: DuplexConn,
}

impl Spotify {
    pub fn new() -> Result<Spotify, rustbus::connection::Error> {
        let mut spotify = Spotify {
            connection: DuplexConn::connect_to_bus(get_session_bus_path()?, true)?,
        };

        spotify.connection.send_hello(DBUS_TIMEOUT)?;

        Ok(spotify)
    }

    pub fn handle_action(&mut self, action: Action) -> Result<(), rustbus::connection::Error> {
        let call = self.call(action.mpris_method());
        self.connection
            .send
            .send_message(&call)?
            .write(DBUS_TIMEOUT)
            .map_err(|(_, err)| err)?;

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
