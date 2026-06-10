use std::env;
use std::error::Error;
use std::io;
use std::mem;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;

mod midi;
mod spotify;

const MAX_MIDI: usize = 3;
const DEFAULT_CLIENT_NAME: &str = "spotify control";
const DEFAULT_PLAY_COMMAND: &str = "176,41,127";
const DEFAULT_PAUSE_COMMAND: &str = "176,42,127";
const DEFAULT_PREVIOUS_COMMAND: &str = "176,58,127";
const DEFAULT_NEXT_COMMAND: &str = "176,59,127";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    Jack,
    PipeWire,
}

struct Config {
    backend: Backend,
    client_name: String,
    pipewire_remote: Option<String>,
    pipewire_target: Option<u32>,
    learning_mode: bool,
    midi_bindings: MidiBindings,
}

impl Config {
    fn from_env_and_args() -> Result<Self, Box<dyn Error>> {
        let mut config = Config {
            backend: env::var("SPOTIFY_MIDI_BACKEND")
                .ok()
                .as_deref()
                .map(parse_backend)
                .transpose()?
                .unwrap_or(Backend::Jack),
            client_name: env::var("SPOTIFY_MIDI_CLIENT_NAME")
                .unwrap_or_else(|_| DEFAULT_CLIENT_NAME.to_string()),
            pipewire_remote: env::var("SPOTIFY_MIDI_PIPEWIRE_REMOTE").ok(),
            pipewire_target: env::var("SPOTIFY_MIDI_PIPEWIRE_TARGET")
                .ok()
                .map(|value| value.parse())
                .transpose()?,
            midi_bindings: MidiBindings::from_env()?,
            learning_mode: parse_bool_env("SPOTIFY_MIDI_LEARN")?,
        };

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--backend" => {
                    let value = args.next().ok_or("--backend requires jack or pipewire")?;
                    config.backend = parse_backend(&value)?;
                }
                "--client-name" => {
                    config.client_name = args.next().ok_or("--client-name requires a value")?;
                }
                "--pipewire-remote" => {
                    config.pipewire_remote =
                        Some(args.next().ok_or("--pipewire-remote requires a value")?);
                }
                "--pipewire-target" => {
                    config.pipewire_target = Some(
                        args.next()
                            .ok_or("--pipewire-target requires a node id")?
                            .parse()?,
                    );
                }
                "--play-command" => {
                    config.midi_bindings.play = parse_midi_command(
                        &args
                            .next()
                            .ok_or("--play-command requires bytes like 176,41,127")?,
                    )?;
                }
                "--pause-command" => {
                    config.midi_bindings.pause = parse_midi_command(
                        &args
                            .next()
                            .ok_or("--pause-command requires bytes like 176,42,127")?,
                    )?;
                }
                "--previous-command" => {
                    config.midi_bindings.previous = parse_midi_command(
                        &args
                            .next()
                            .ok_or("--previous-command requires bytes like 176,58,127")?,
                    )?;
                }
                "--next-command" => {
                    config.midi_bindings.next = parse_midi_command(
                        &args
                            .next()
                            .ok_or("--next-command requires bytes like 176,59,127")?,
                    )?;
                }
                "--learn" => {
                    config.learning_mode = true;
                }
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                unknown => return Err(format!("unknown argument: {unknown}").into()),
            }
        }

        Ok(config)
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
    fn from_env() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            play: parse_midi_command_env("SPOTIFY_MIDI_PLAY_COMMAND", DEFAULT_PLAY_COMMAND)?,
            pause: parse_midi_command_env("SPOTIFY_MIDI_PAUSE_COMMAND", DEFAULT_PAUSE_COMMAND)?,
            previous: parse_midi_command_env(
                "SPOTIFY_MIDI_PREVIOUS_COMMAND",
                DEFAULT_PREVIOUS_COMMAND,
            )?,
            next: parse_midi_command_env("SPOTIFY_MIDI_NEXT_COMMAND", DEFAULT_NEXT_COMMAND)?,
        })
    }

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

fn parse_midi_command_env(name: &str, default: &str) -> Result<MidiCommand, Box<dyn Error>> {
    let value = env::var(name).unwrap_or_else(|_| default.to_string());
    parse_midi_command(&value)
}

fn parse_midi_command(value: &str) -> Result<MidiCommand, Box<dyn Error>> {
    let parts: Vec<&str> = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() || parts.len() > MAX_MIDI {
        return Err(format!("MIDI command {value:?} must contain 1 to {MAX_MIDI} bytes").into());
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

fn parse_midi_byte(value: &str) -> Result<u8, Box<dyn Error>> {
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u8::from_str_radix(hex, 16)?
    } else if let Some(binary) = value.strip_prefix("0b") {
        u8::from_str_radix(binary, 2)?
    } else {
        value.parse()?
    };
    Ok(parsed)
}

fn parse_bool_env(name: &str) -> Result<bool, Box<dyn Error>> {
    match env::var(name)
        .ok()
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("") | Some("0") | Some("false") | Some("no") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("yes") | Some("on") => Ok(true),
        Some(value) => Err(format!("{name} must be true or false, got {value:?}").into()),
    }
}

fn parse_backend(value: &str) -> Result<Backend, Box<dyn Error>> {
    match value.to_ascii_lowercase().as_str() {
        "jack" => Ok(Backend::Jack),
        "pipewire" | "pw" => Ok(Backend::PipeWire),
        _ => Err(format!("unsupported backend {value:?}; expected jack or pipewire").into()),
    }
}

fn print_help() {
    println!(
        "spotify-midi-control\n\n  --backend <jack|pipewire>\n  --client-name <name>\n  --pipewire-remote <remote>\n  --pipewire-target <node-id>\n  --learn\n  --play-command <bytes>\n  --pause-command <bytes>\n  --previous-command <bytes>\n  --next-command <bytes>\n\nMIDI command bytes are comma-separated, for example 176,41,127 or 0xB0,41,127.\n\nEnvironment equivalents:\n  SPOTIFY_MIDI_BACKEND\n  SPOTIFY_MIDI_CLIENT_NAME\n  SPOTIFY_MIDI_PIPEWIRE_REMOTE\n  SPOTIFY_MIDI_PIPEWIRE_TARGET\n  SPOTIFY_MIDI_LEARN\n  SPOTIFY_MIDI_PLAY_COMMAND\n  SPOTIFY_MIDI_PAUSE_COMMAND\n  SPOTIFY_MIDI_PREVIOUS_COMMAND\n  SPOTIFY_MIDI_NEXT_COMMAND"
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::from_env_and_args()?;

    let (sender, receiver) = sync_channel(64);
    let _controller = if config.learning_mode {
        spawn_learning_controller(receiver)
    } else {
        let spotify_sender = sender.clone();
        spawn_controller(receiver, spotify_sender, config.midi_bindings)
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
