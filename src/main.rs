use std::error::Error;
use std::io::{self, IsTerminal};
use std::mem;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::{Parser, ValueEnum};

mod midi;
mod rt_midi_queue;
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

    #[arg(long, env = "SPOTIFY_MIDI_VERBOSE")]
    verbose: bool,

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
    verbose: bool,
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
            verbose: args.verbose,
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
    fn action_for(&self, midi: &midi::MidiCopy) -> Option<spotify::Action> {
        if self.play.matches(midi) {
            Some(spotify::Action::Play)
        } else if self.pause.matches(midi) {
            Some(spotify::Action::Pause)
        } else if self.previous.matches(midi) {
            Some(spotify::Action::Previous)
        } else if self.next.matches(midi) {
            Some(spotify::Action::Next)
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
    let (rt_producer, rt_consumer) = rt_midi_queue::channel(256);
    let drop_counters = MidiDropCounters::default();
    let _drop_reporter = spawn_drop_reporter(drop_counters.clone());
    let _midi_forwarder = spawn_midi_forwarder(rt_consumer, sender.clone(), drop_counters.clone());
    let _controller = if config.learning_mode {
        spawn_learning_controller(receiver)
    } else {
        spawn_controller(
            receiver,
            config
                .midi_bindings
                .expect("MIDI bindings are validated before startup"),
            config.verbose,
        )
    };

    match config.backend {
        Backend::Jack => run_jack(&config.client_name, rt_producer, drop_counters)?,
        Backend::PipeWire => run_pipewire(&config, rt_producer, drop_counters)?,
    }

    Ok(())
}

#[derive(Clone, Default)]
struct MidiDropCounters {
    realtime_queue: Arc<AtomicU64>,
    forwarder_channel: Arc<AtomicU64>,
}

fn drain_drop_counts(counters: &MidiDropCounters) -> (u64, u64) {
    (
        counters.realtime_queue.swap(0, Ordering::Relaxed),
        counters.forwarder_channel.swap(0, Ordering::Relaxed),
    )
}

fn spawn_drop_reporter(counters: MidiDropCounters) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(30));
        let (rt_drops, forwarder_drops) = drain_drop_counts(&counters);
        if rt_drops != 0 || forwarder_drops != 0 {
            eprintln!(
                "dropped MIDI events: realtime_queue={rt_drops} forwarder_channel={forwarder_drops}"
            );
        }
    })
}

fn spawn_midi_forwarder(
    mut consumer: rt_midi_queue::Consumer,
    sender: SyncSender<midi::MidiCopy>,
    drop_counters: MidiDropCounters,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        while let Some(midi) = consumer.try_pop() {
            if sender.try_send(midi).is_err() {
                drop_counters
                    .forwarder_channel
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        thread::sleep(Duration::from_millis(1));
    })
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
    midi_bindings: MidiBindings,
    verbose: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut spot = match spotify::Spotify::new() {
            Ok(spot) => spot,
            Err(err) => {
                eprintln!("failed to connect to session bus: {err}");
                return;
            }
        };

        while let Ok(m) = receiver.recv() {
            if verbose {
                println!("midi data: {:?}", m);
            }
            if let Some(action) = midi_bindings.action_for(&m) {
                if let Err(err) = spot.handle_action(action) {
                    eprintln!("failed to send Spotify action {action:?}: {err}");
                }
            }
        }
    })
}

fn run_jack(
    client_name: &str,
    mut producer: rt_midi_queue::Producer,
    drop_counters: MidiDropCounters,
) -> Result<(), Box<dyn Error>> {
    let (client, _status) = jack::Client::new(client_name, jack::ClientOptions::NO_START_SERVER)?;
    let midi_in = client.register_port("MIDI In", jack::MidiIn)?;

    let callback = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        for event in midi_in.iter(ps) {
            if producer.try_push(event.into()).is_err() {
                drop_counters.realtime_queue.fetch_add(1, Ordering::Relaxed);
            }
        }
        jack::Control::Continue
    };

    let active_client = client.activate_async((), jack::ClosureProcessHandler::new(callback))?;
    wait_for_quit();
    active_client.deactivate()?;

    Ok(())
}

fn wait_for_quit() {
    if io::stdin().is_terminal() {
        println!("Press enter to quit");
        let mut user_input = String::new();
        io::stdin().read_line(&mut user_input).ok();
    } else {
        println!("Running until service stop");
        loop {
            thread::park();
        }
    }
}

fn run_pipewire(
    config: &Config,
    producer: rt_midi_queue::Producer,
    drop_counters: MidiDropCounters,
) -> Result<(), Box<dyn Error>> {
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

    let props = pipewire_stream_props(config.pipewire_target);
    let stream = pw::stream::StreamBox::new(&core, &config.client_name, props)?;
    let _listener = stream
        .add_local_listener_with_user_data((producer, drop_counters))
        .state_changed(|_, _, old, new| {
            println!("PipeWire state changed: {:?} -> {:?}", old, new);
        })
        .process(|stream, (producer, drop_counters)| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            for_each_pipewire_midi_message(&mut buffer, |message| {
                if producer.try_push(message).is_err() {
                    drop_counters.realtime_queue.fetch_add(1, Ordering::Relaxed);
                }
            });
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
        pipewire_stream_flags(),
        &mut params,
    )?;

    let quit_receiver = if io::stdin().is_terminal() {
        println!("Press enter to quit");
        let (quit_sender, quit_receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut user_input = String::new();
            io::stdin().read_line(&mut user_input).ok();
            let _ = quit_sender.send(());
        });
        Some(quit_receiver)
    } else {
        println!("Running until service stop");
        None
    };

    loop {
        mainloop.loop_().iterate(pw::loop_::Timeout::Finite(
            std::time::Duration::from_millis(100),
        ));

        if quit_receiver
            .as_ref()
            .is_some_and(|receiver| receiver.try_recv().is_ok())
        {
            break;
        }
    }

    Ok(())
}

fn pipewire_stream_flags() -> pipewire::stream::StreamFlags {
    use pipewire as pw;

    pw::stream::StreamFlags::MAP_BUFFERS
        | pw::stream::StreamFlags::DONT_RECONNECT
        | pw::stream::StreamFlags::RT_PROCESS
}

fn pipewire_stream_props(pipewire_target: Option<u32>) -> pipewire::properties::PropertiesBox {
    use pipewire as pw;
    use pw::properties::properties;

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Midi",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Control",
        *pw::keys::NODE_AUTOCONNECT => "false",
        *pw::keys::NODE_DONT_RECONNECT => "true",
    };
    if let Some(target) = pipewire_target {
        props.insert("target.object", target.to_string());
    }

    props
}

fn for_each_pipewire_midi_message(
    buffer: &mut pipewire::buffer::Buffer<'_>,
    mut handler: impl FnMut(midi::MidiCopy),
) {
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

        for_each_spa_midi_sequence(&bytes[start..end], &mut handler);
    }
}

fn for_each_spa_midi_sequence(bytes: &[u8], handler: &mut impl FnMut(midi::MidiCopy)) {
    let Some(pod_header) = read_unaligned::<libspa::sys::spa_pod>(bytes, 0) else {
        return;
    };
    if pod_header.type_ != libspa::sys::SPA_TYPE_Sequence {
        return;
    }

    let pod_header_size = mem::size_of::<libspa::sys::spa_pod>();
    let pod_size = pod_header.size as usize;
    let Some(pod_end) = pod_header_size.checked_add(pod_size) else {
        return;
    };
    let Some(body) = bytes.get(pod_header_size..pod_end) else {
        return;
    };

    let sequence_body_size = mem::size_of::<libspa::sys::spa_pod_sequence_body>();
    if body.len() < sequence_body_size {
        return;
    }

    let mut offset = sequence_body_size;
    let control_header_size = mem::size_of::<libspa::sys::spa_pod_control>();
    while offset
        .checked_add(control_header_size)
        .is_some_and(|control_end| control_end <= body.len())
    {
        let Some(control) = read_unaligned::<libspa::sys::spa_pod_control>(body, offset) else {
            break;
        };
        let value_size = control.value.size as usize;
        let value_type = control.value.type_;

        let Some(value_start) = offset.checked_add(control_header_size) else {
            break;
        };
        let Some(value_end) = value_start.checked_add(value_size) else {
            break;
        };
        let Some(value) = body.get(value_start..value_end) else {
            break;
        };

        if control.type_ == libspa::sys::SPA_CONTROL_Midi
            && value_type == libspa::sys::SPA_TYPE_Bytes
        {
            handler(midi::MidiCopy::from_bytes(value, control.offset));
        }

        let Some(control_size) = control_header_size.checked_add(value_size) else {
            break;
        };
        let Some(aligned_control_size) = align_pod_size(control_size) else {
            break;
        };
        let Some(next_offset) = offset.checked_add(aligned_control_size) else {
            break;
        };
        offset = next_offset;
    }
}

fn read_unaligned<T: Copy>(bytes: &[u8], offset: usize) -> Option<T> {
    let end = offset.checked_add(mem::size_of::<T>())?;
    let src = bytes.get(offset..end)?;
    Some(unsafe { std::ptr::read_unaligned(src.as_ptr().cast::<T>()) })
}

fn align_pod_size(size: usize) -> Option<usize> {
    Some(size.checked_add(7)? & !7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_midi_command_accepts_one_to_three_bytes() {
        assert_eq!(parse_midi_command("176").unwrap().len, 1);
        assert_eq!(parse_midi_command("176,41").unwrap().len, 2);
        assert_eq!(
            parse_midi_command("0xB0,0b101001,127").unwrap().bytes,
            [176, 41, 127]
        );
    }

    #[test]
    fn parse_midi_command_rejects_empty_or_too_long_commands() {
        assert!(parse_midi_command("").is_err());
        assert!(parse_midi_command("176,41,127,1").is_err());
    }

    #[test]
    fn midi_bindings_return_typed_spotify_actions() {
        let bindings = MidiBindings {
            play: parse_midi_command("176,41,127").unwrap(),
            pause: parse_midi_command("176,42,127").unwrap(),
            previous: parse_midi_command("176,58,127").unwrap(),
            next: parse_midi_command("176,59,127").unwrap(),
        };

        assert_eq!(
            bindings.action_for(&midi::MidiCopy::from_bytes(&[176, 41, 127], 0)),
            Some(spotify::Action::Play)
        );
        assert_eq!(
            bindings.action_for(&midi::MidiCopy::from_bytes(&[176, 42, 127], 0)),
            Some(spotify::Action::Pause)
        );
        assert_eq!(
            bindings.action_for(&midi::MidiCopy::from_bytes(&[176, 58, 127], 0)),
            Some(spotify::Action::Previous)
        );
        assert_eq!(
            bindings.action_for(&midi::MidiCopy::from_bytes(&[176, 59, 127], 0)),
            Some(spotify::Action::Next)
        );
        assert_eq!(
            bindings.action_for(&midi::MidiCopy::from_bytes(&[176, 99, 127], 0)),
            None
        );
    }

    #[test]
    fn drain_drop_counts_reports_and_resets_counts() {
        let counters = MidiDropCounters::default();
        counters.realtime_queue.fetch_add(2, Ordering::Relaxed);
        counters.forwarder_channel.fetch_add(3, Ordering::Relaxed);

        assert_eq!(drain_drop_counts(&counters), (2, 3));
        assert_eq!(drain_drop_counts(&counters), (0, 0));
    }

    #[test]
    fn spa_midi_sequence_ignores_non_sequence_pod() {
        let bytes = test_pod(libspa::sys::SPA_TYPE_Int, &[1, 0, 0, 0]);
        let mut events = Vec::new();

        for_each_spa_midi_sequence(&bytes, &mut |event| events.push(event));

        assert!(events.is_empty());
    }

    #[test]
    fn spa_midi_sequence_ignores_truncated_sequence_body() {
        let bytes = test_pod(libspa::sys::SPA_TYPE_Sequence, &[0, 0, 0, 0]);
        let mut events = Vec::new();

        for_each_spa_midi_sequence(&bytes, &mut |event| events.push(event));

        assert!(events.is_empty());
    }

    #[test]
    fn spa_midi_sequence_ignores_truncated_control() {
        let mut body = Vec::new();
        append_plain(
            &mut body,
            &libspa::sys::spa_pod_sequence_body { unit: 0, pad: 0 },
        );
        body.extend_from_slice(&[1, 2, 3, 4]);
        let bytes = test_pod(libspa::sys::SPA_TYPE_Sequence, &body);
        let mut events = Vec::new();

        for_each_spa_midi_sequence(&bytes, &mut |event| events.push(event));

        assert!(events.is_empty());
    }

    #[test]
    fn spa_midi_sequence_ignores_oversized_control_value() {
        let mut body = Vec::new();
        append_plain(
            &mut body,
            &libspa::sys::spa_pod_sequence_body { unit: 0, pad: 0 },
        );
        append_plain(
            &mut body,
            &libspa::sys::spa_pod_control {
                offset: 7,
                type_: libspa::sys::SPA_CONTROL_Midi,
                value: libspa::sys::spa_pod {
                    size: 10,
                    type_: libspa::sys::SPA_TYPE_Bytes,
                },
            },
        );
        body.push(176);
        let bytes = test_pod(libspa::sys::SPA_TYPE_Sequence, &body);
        let mut events = Vec::new();

        for_each_spa_midi_sequence(&bytes, &mut |event| events.push(event));

        assert!(events.is_empty());
    }

    #[test]
    fn spa_midi_sequence_emits_valid_midi_control() {
        let bytes = test_midi_sequence(&[176, 41, 127], 11);
        let mut events = Vec::new();

        for_each_spa_midi_sequence(&bytes, &mut |event| events.push(event));

        assert_eq!(
            events,
            vec![midi::MidiCopy::from_bytes(&[176, 41, 127], 11)]
        );
    }

    fn test_midi_sequence(value: &[u8], offset: u32) -> Vec<u8> {
        let mut body = Vec::new();
        append_plain(
            &mut body,
            &libspa::sys::spa_pod_sequence_body { unit: 0, pad: 0 },
        );
        append_plain(
            &mut body,
            &libspa::sys::spa_pod_control {
                offset,
                type_: libspa::sys::SPA_CONTROL_Midi,
                value: libspa::sys::spa_pod {
                    size: value.len() as u32,
                    type_: libspa::sys::SPA_TYPE_Bytes,
                },
            },
        );
        body.extend_from_slice(value);
        let padding = align_pod_size(mem::size_of::<libspa::sys::spa_pod_control>() + value.len())
            .unwrap()
            - mem::size_of::<libspa::sys::spa_pod_control>()
            - value.len();
        body.resize(body.len() + padding, 0);

        test_pod(libspa::sys::SPA_TYPE_Sequence, &body)
    }

    fn test_pod(type_: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_plain(
            &mut bytes,
            &libspa::sys::spa_pod {
                size: body.len() as u32,
                type_,
            },
        );
        bytes.extend_from_slice(body);
        bytes
    }

    fn append_plain<T: Copy>(bytes: &mut Vec<u8>, value: &T) {
        let value_bytes = unsafe {
            std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
        };
        bytes.extend_from_slice(value_bytes);
    }

    #[test]
    fn pipewire_stream_props_do_not_request_live_driver_scheduling() {
        let props = pipewire_stream_props(None);

        assert_eq!(props.get("media.type"), Some("Midi"));
        assert_eq!(props.get("media.category"), Some("Capture"));
        assert_eq!(props.get("media.role"), Some("Control"));
        assert_eq!(props.get("stream.is-live"), None);
        assert_eq!(props.get("node.want-driver"), None);
        assert_eq!(props.get("node.autoconnect"), Some("false"));
        assert_eq!(props.get("node.dont-reconnect"), Some("true"));
    }

    #[test]
    fn realtime_midi_queue_preserves_events_without_blocking() {
        let (mut producer, mut consumer) = rt_midi_queue::channel(2);
        let first = midi::MidiCopy::from_bytes(&[176, 41, 127], 0);
        let second = midi::MidiCopy::from_bytes(&[176, 42, 0], 1);

        assert!(producer.try_push(first).is_ok());
        assert!(producer.try_push(second).is_ok());
        assert!(producer.try_push(first).is_err());

        assert_eq!(consumer.try_pop(), Some(first));
        assert_eq!(consumer.try_pop(), Some(second));
        assert_eq!(consumer.try_pop(), None);
    }

    #[test]
    fn pipewire_stream_flags_enable_realtime_processing() {
        let flags = pipewire_stream_flags();

        assert!(flags.contains(pipewire::stream::StreamFlags::RT_PROCESS));
        assert!(flags.contains(pipewire::stream::StreamFlags::MAP_BUFFERS));
        assert!(flags.contains(pipewire::stream::StreamFlags::DONT_RECONNECT));
    }

    #[test]
    fn pipewire_stream_props_preserve_explicit_target() {
        let props = pipewire_stream_props(Some(123));

        assert_eq!(props.get("target.object"), Some("123"));
    }
}
