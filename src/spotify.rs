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
    pub(crate) fn mpris_method(self) -> &'static str {
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
        let call = call(action);
        self.connection
            .send
            .send_message(&call)?
            .write(DBUS_TIMEOUT)
            .map_err(|(_, err)| err)?;

        Ok(())
    }
}

fn call(action: Action) -> MarshalledMessage {
    MessageBuilder::new()
        .call(action.mpris_method())
        .with_interface("org.mpris.MediaPlayer2.Player")
        .on(SPOTIFY_PATH)
        .at(SPOTIFY_DST)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustbus::message_builder::MessageType;

    #[test]
    fn actions_map_to_mpris_method_names() {
        assert_eq!(Action::Play.mpris_method(), "Play");
        assert_eq!(Action::Pause.mpris_method(), "Pause");
        assert_eq!(Action::Previous.mpris_method(), "Previous");
        assert_eq!(Action::Next.mpris_method(), "Next");
    }

    #[test]
    fn call_builds_mpris_player_method_call() {
        let message = call(Action::Next);

        assert_eq!(message.typ, MessageType::Call);
        assert_eq!(message.dynheader.member.as_deref(), Some("Next"));
        assert_eq!(
            message.dynheader.interface.as_deref(),
            Some("org.mpris.MediaPlayer2.Player")
        );
        assert_eq!(message.dynheader.object.as_deref(), Some(SPOTIFY_PATH));
        assert_eq!(message.dynheader.destination.as_deref(), Some(SPOTIFY_DST));
        assert_eq!(message.get_sig(), "");
        assert!(message.get_buf().is_empty());
    }
}
