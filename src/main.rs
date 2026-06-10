use std::error::Error;
use std::io;
use std::mem;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;

use clap::{Parser, ValueEnum};

mod midi;
mod spotify;

const MAX_MIDI: usize = 3;
const DEFAULT_CLIENT_NAME: &str = "spotify control";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Backend {
    Jack,
    #[value(name = "pipewire", alias = "pw")]
    PipeWire,
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long, value_enum, env = "SPOTIFY_MIDI_BACKEND", default_value = "jack")]
    backend: Backend,

    #[arg(long, env = "SPOTIFY_MIDI_CLIENT_NAME", default_value = DEFAULT_CLIENT_NAME)]
    client_name: String,

    #[arg(long, env = "SPOTIFY_MIDI_PIPEWIRE_REMOTE")]
    pipewire_remote: Option<String>,

    #[arg(long, env = "SPOTIFY_MIDI_PIPEWIRE_TARGET")]
    pipewire_target: Option<u32>,

    #[arg(long, env = "SPOTIFY_MIDI_LEARN")]
    learn: bool,

    #[arg(long, env = "SPOTIFY_MIDI_PLAY_COMMAND", value_parser = parse_midi_command, required_unless_present = "learn")]
    play_command: Option<MidiCommand>,

    #[arg(long, env = "SPOTIFY_MIDI_PAUSE_COMMAND", value_parser = parse_midi_command, required_unless_present = "learn")]
    pause_command: Option<MidiCommand>,

    #[arg(long, env = "SPOTIFY_MIDI_PREVIOUS_COMMAND", value_parser = parse_midi_command, required_unless_present = "learn")]
    previous_command: Option<MidiCommand>,

    #[arg(long, env = "SPOTIFY_MIDI_NEXT_COMMAND", value_parser = parse_midi_command, required_unless_present = "learn")]
    next_command: Option<MidiCommand>,
}

struct Config {
    backend: Backend,
    client_name: String,
    pipewire_remote: Option<String>,
    pipewire_target: Option<u32>,
    learning_mode: bool,
    midi_bindings: Option<MidiBindings>,
}

impl Config {
    fn from_args() -> Result<Self, Box<dyn Error>> {
        let args = Args::parse();
        let midi_bindings = if args.learn {
            None
        } else {
            Some(MidiBindings {
                play: args
                    .play_command
                    .ok_or("--play-command or SPOTIFY_MIDI_PLAY_COMMAND is required")?,
                pause: args
                    .pause_command
                    .ok_or("--pause-command or SPOTIFY_MIDI_PAUSE_COMMAND is required")?,
                previous: args
                    .previous_command
                    .ok_or("--previous-command or SPOTIFY_MIDI_PREVIOUS_COMMAND is required")?,
                next: args
                    .next_command
                    .ok_or("--next-command or SPOTIFY_MIDI_NEXT_COMMAND is required")?,
            })
        };

        Ok(Self {
            backend: args.backend,
            client_name: args.client_name,
            pipewire_remote: args.pipewire_remote,
            pipewire_target: args.pipewire_target,
            learning_mode: args.learn,
            midi_bindings,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MidiCommand {
    bytes: [u8; MAX_MIDI],
    len: usize,
}

impl MidiCommand {
    fn matches(&self, midi: &midi::MidiCopy) -> bool {
        midi.len == self.len && midi.data[..self.len] == self.bytes[..self.len]
    }
}

#[derive(Clone, Copy, Debug)]
struct MidiBindings {
    play: MidiCommand,
    pause: MidiCommand,
    previous: MidiCommand,
    next: MidiCommand,
}

impl MidiBindings {
    fn action_for(&self, midi: &midi::MidiCopy) -> Option<&str> {
        if self.play.matches(midi) {
            Some("Play")
        } else if self.pause.matches(midi) {
            Some("Pause")
        } else if self.previous.matches(midi) {
            Some("Previous")
        } else if self.next.matches(midi) {
            Some("Next")
        } else {
            None
        }
    }
}

fn parse_midi_command(value: &str) -> Result<MidiCommand, String> {
    let parts: Vec<&str> = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() || parts.len() > MAX_MIDI {
        return Err(format!(
            "MIDI command {value:?} must contain 1 to {MAX_MIDI} bytes"
        ));
    }

    let mut bytes = [0; MAX_MIDI];
    for (index, part) in parts.iter().enumerate() {
        bytes[index] = parse_midi_byte(part)?;
    }

    Ok(MidiCommand {
        bytes,
        len: parts.len(),
    })
}

fn parse_midi_byte(value: &str) -> Result<u8, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        u8::from_str_radix(hex, 16).map_err(|err| format!("invalid MIDI byte {value:?}: {err}"))
    } else if let Some(binary) = value.strip_prefix("0b") {
        u8::from_str_radix(binary, 2).map_err(|err| format!("invalid MIDI byte {value:?}: {err}"))
    } else {
        value
            .parse()
            .map_err(|err| format!("invalid MIDI byte {value:?}: {err}"))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::from_args()?;

    let (sender, receiver) = sync_channel(64);
    let _controller = if config.learning_mode {
        spawn_learning_controller(receiver)
    } else {
        let spotify_sender = sender.clone();
        spawn_controller(
            receiver,
            spotify_sender,
            config
                .midi_bindings
                .expect("MIDI bindings are validated before startup"),
        )
    };

    match config.backend {
        Backend::Jack => run_jack(&config.client_name, sender)?,
        Backend::PipeWire => run_pipewire(&config, sender)?,
    }

    Ok(())
}

fn spawn_learning_controller(receiver: Receiver<midi::MidiCopy>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        println!("Learning mode: press MIDI controls, then use the printed bytes in your config.");
        while let Ok(midi) = receiver.recv() {
            let command = midi_command_string(&midi);
            println!(
                "midi command: {command}    nix: [ {} ]    status: {} channel: {}",
                midi_command_nix_list(&midi),
                midi.status,
                midi.channel
            );
        }
    })
}

fn midi_command_string(midi: &midi::MidiCopy) -> String {
    midi.data[..midi.len]
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn midi_command_nix_list(midi: &midi::MidiCopy) -> String {
    midi.data[..midi.len]
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn spawn_controller(
    receiver: Receiver<midi::MidiCopy>,
    spotify_sender: SyncSender<midi::MidiCopy>,
    midi_bindings: MidiBindings,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut spot = match spotify::Spotify::new(spotify_sender) {
            Ok(spot) => spot,
            Err(err) => {
                eprintln!("failed to connect to session bus: {err}");
                return;
            }
        };

        while let Ok(m) = receiver.recv() {
            println!("midi data: {:?}", m);
            if let Some(action) = midi_bindings.action_for(&m) {
                if let Err(err) = spot.handle_midi(m, action) {
                    eprintln!("failed to send Spotify action {action}: {err}");
                }
            }
        }
    })
}

fn run_jack(client_name: &str, sender: SyncSender<midi::MidiCopy>) -> Result<(), Box<dyn Error>> {
    let (client, _status) = jack::Client::new(client_name, jack::ClientOptions::NO_START_SERVER)?;
    let midi_in = client.register_port("MIDI In", jack::MidiIn)?;

    let callback = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        for event in midi_in.iter(ps) {
            let _ = sender.try_send(event.into());
        }
        jack::Control::Continue
    };

    let active_client = client.activate_async((), jack::ClosureProcessHandler::new(callback))?;
    wait_for_quit();
    active_client.deactivate()?;

    Ok(())
}

fn wait_for_quit() {
    println!("Press enter to quit");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
}

fn run_pipewire(config: &Config, sender: SyncSender<midi::MidiCopy>) -> Result<(), Box<dyn Error>> {
    use libspa::pod::Pod;
    use pipewire as pw;
    use pw::properties::properties;
    use pw::spa;
    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    use spa::pod::serialize::PodSerializer;

    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core_props = config.pipewire_remote.as_ref().map(|remote| {
        properties! {
            *pw::keys::REMOTE_NAME => remote.as_str(),
        }
    });
    let core = context.connect_rc(core_props)?;

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Midi",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
    };
    if let Some(target) = config.pipewire_target {
        props.insert("target.object", target.to_string());
    }

    let stream = pw::stream::StreamBox::new(&core, &config.client_name, props)?;
    let _listener = stream
        .add_local_listener_with_user_data(sender)
        .state_changed(|_, _, old, new| {
            println!("PipeWire state changed: {:?} -> {:?}", old, new);
        })
        .process(|stream, sender| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            for message in pipewire_midi_messages(&mut buffer) {
                let _ = sender.try_send(message);
            }
        })
        .register()?;

    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Application),
        spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Control)
    );
    let values: Vec<u8> = PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or("failed to build PipeWire MIDI format")?];

    stream.connect(
        spa::utils::Direction::Input,
        config.pipewire_target,
        pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    println!("Press enter to quit");
    let (quit_sender, quit_receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut user_input = String::new();
        io::stdin().read_line(&mut user_input).ok();
        let _ = quit_sender.send(());
    });

    while quit_receiver.try_recv().is_err() {
        mainloop.loop_().iterate(pw::loop_::Timeout::Finite(
            std::time::Duration::from_millis(100),
        ));
    }

    Ok(())
}

fn pipewire_midi_messages(buffer: &mut pipewire::buffer::Buffer<'_>) -> Vec<midi::MidiCopy> {
    let mut messages = Vec::new();

    for data in buffer.datas_mut() {
        let start = data.chunk().offset() as usize;
        let size = data.chunk().size() as usize;
        let Some(bytes) = data.data() else {
            continue;
        };
        let end = start.saturating_add(size).min(bytes.len());
        if start >= end {
            continue;
        }

        messages.extend(parse_spa_midi_sequence(&bytes[start..end]));
    }

    messages
}

fn parse_spa_midi_sequence(bytes: &[u8]) -> Vec<midi::MidiCopy> {
    let mut messages = Vec::new();
    let Some(pod) = libspa::pod::Pod::from_bytes(bytes) else {
        return messages;
    };
    if pod.type_() != libspa::utils::SpaTypes::Sequence {
        return messages;
    }

    let pod_size = pod.size() as usize;
    let sequence_body_size = mem::size_of::<libspa::sys::spa_pod_sequence_body>();
    if pod_size < sequence_body_size {
        return messages;
    }

    let mut offset = sequence_body_size;
    while offset + mem::size_of::<libspa::sys::spa_pod_control>() <= pod_size {
        let control = unsafe {
            &*(pod.body().cast::<u8>().add(offset) as *const libspa::sys::spa_pod_control)
        };
        let value_size = control.value.size as usize;
        let value_type = control.value.type_;

        let control_size = mem::size_of::<libspa::sys::spa_pod_control>() + value_size;
        if offset + control_size > pod_size {
            break;
        }

        if control.type_ == libspa::sys::SPA_CONTROL_Midi
            && value_type == libspa::sys::SPA_TYPE_Bytes
        {
            let value = unsafe {
                std::slice::from_raw_parts(
                    (&control.value as *const libspa::sys::spa_pod)
                        .add(1)
                        .cast::<u8>(),
                    value_size,
                )
            };
            messages.push(midi::MidiCopy::from_bytes(value, control.offset));
        }

        offset += align_pod_size(control_size);
    }

    messages
}

fn align_pod_size(size: usize) -> usize {
    (size + 7) & !7
}
