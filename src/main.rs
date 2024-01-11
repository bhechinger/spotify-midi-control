use std::io;
use std::sync::mpsc::sync_channel;

mod spotify;
mod midi;

const MAX_MIDI: usize = 3;

//a fixed size container to copy data out of real-time thread


fn main() -> Result<(), rustbus::connection::Error> {
    let (client, _status) =
        jack::Client::new("spotify control", jack::ClientOptions::NO_START_SERVER).unwrap();

    //create a sync channel to send back copies of midi messages we get
    let (sender, receiver) = sync_channel(64);
    let sender2 = sender.clone();

    // process logic
    let mut _maker = client
        .register_port("MIDI Through", jack::MidiOut)
        .unwrap();
    let shower = client
        .register_port("MIDI In", jack::MidiIn)
        .unwrap();

    let callback = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let show_p = shower.iter(ps);
        for e in show_p {
            let c: midi::MidiCopy = e.into();
            let _ = sender.try_send(c);
        }
        jack::Control::Continue
    };

    // activate
    let active_client = client
        .activate_async((), jack::ClosureProcessHandler::new(callback))
        .unwrap();

    let mut spot = spotify::Spotify::new(sender2)?;

    // MIDI notes:
    // Play 41
    // Stop 42
    // Track Back 58
    // Track Forward 59
    //spawn a non-real-time thread that sends the dbus messages to Spotify
    std::thread::spawn(move || {
        while let Ok(m) = receiver.recv() {
            if m.data[2] == 127 {
                match m.data[1] {
                    41 => spot.handle_midi(m, "Play").unwrap(),
                    42 => spot.handle_midi(m, "Pause").unwrap(),
                    58 => spot.handle_midi(m, "Previous").unwrap(),
                    59 => spot.handle_midi(m, "Next").unwrap(),
                    _ => spot.handle_midi(m, "pass-through").unwrap()
                }
            } else {
                match m.data[1] {
                    41 | 42 | 58 | 59 => (), // Ignore these, we only want the NoteOn message
                    _ => spot.handle_midi(m, "pass-through").unwrap()
                }
            }

        }
    });

    // wait
    println!("Press any key to quit");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();

    // optional deactivation
    active_client.deactivate().unwrap();

    Ok(())
}