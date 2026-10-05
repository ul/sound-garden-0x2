use anyhow::Result;
use audio_server::{
    Message, MidiInputSelection, Monitor, Options, list_audio_outputs, list_midi_inputs,
    run_with_options,
};
use clap::{Arg, Command, crate_authors, crate_description, crate_name, crate_version};
use crossbeam_channel::{Receiver, Sender};
use rkyv::{from_bytes, rancor::Error};
use std::io::{Read, Write};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;

fn main() -> Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()?;
    let matches = Command::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT")
                .default_value("31337")
                .help("Port to listen to for programs."),
        )
        .arg(
            Arg::new("scope-port")
                .short('o')
                .long("scope-port")
                .value_name("SCOPE_PORT")
                .help("Port to send oscilloscope samples."),
        )
        .arg(Arg::new("midi").long("midi").value_name("DEVICE").help(
            "Connect a MIDI input: 'auto', device index, or case-insensitive name substring.",
        ))
        .arg(
            Arg::new("audio-device")
                .long("audio-device")
                .value_name("DEVICE")
                .help("Audio output: index from --list-audio or case-insensitive name substring. Default: the system's."),
        )
        .arg(
            Arg::new("sample-rate")
                .long("sample-rate")
                .value_name("HZ")
                .value_parser(clap::value_parser!(u32).range(8000..=384000))
                .help("Sample rate in Hz, e.g. 96000; falls back to the device's own if unsupported. On macOS this sets the device's rate system-wide. Default: the device's own."),
        )
        .arg(
            Arg::new("buffer")
                .long("buffer")
                .value_name("FRAMES")
                .value_parser(clap::value_parser!(u32).range(16..=8192))
                .help("Audio buffer size in frames, e.g. 128 for low latency; clamped to what the device supports. Default: the device's own."),
        )
        .arg(
            Arg::new("list-audio")
                .long("list-audio")
                .action(clap::ArgAction::SetTrue)
                .help("List audio output devices with their supported sample rates and exit."),
        )
        .arg(
            Arg::new("list-midi")
                .long("list-midi")
                .action(clap::ArgAction::SetTrue)
                .help("List available MIDI input devices and exit."),
        )
        .get_matches();

    if matches.get_flag("list-audio") {
        for line in list_audio_outputs()? {
            println!("{line}");
        }
        return Ok(());
    }

    if matches.get_flag("list-midi") {
        for line in list_midi_inputs()? {
            println!("{line}");
        }
        return Ok(());
    }

    let scope_port = matches
        .get_one::<String>("scope-port")
        .and_then(|s| s.parse::<u16>().ok());
    let midi = matches
        .get_one::<String>("midi")
        .map(|selection| {
            if selection == "auto" {
                MidiInputSelection::Auto
            } else {
                MidiInputSelection::Match(selection.clone())
            }
        })
        .unwrap_or_default();
    let audio_device = matches.get_one::<String>("audio-device").cloned();
    let sample_rate = matches.get_one::<u32>("sample-rate").copied();
    let buffer_frames = matches.get_one::<u32>("buffer").copied();
    let worker = Worker::spawn("Synth", CHANNEL_CAPACITY, move |rx, tx| {
        run_with_options(
            rx,
            tx,
            Options {
                midi,
                audio_device,
                sample_rate,
                buffer_frames,
            },
        );
    });

    let oscilloscope = if let Some(port) = scope_port {
        Worker::spawn(
            "Oscilloscope (tcp)",
            CHANNEL_CAPACITY,
            move |rx: Receiver<Monitor>, _: Sender<()>| {
                let address = format!("127.0.0.1:{port}");
                let mut stream = std::net::TcpStream::connect(&address).ok();
                for monitor in rx {
                    let frame = monitor.scope;
                    if stream.is_none() {
                        stream = std::net::TcpStream::connect(&address).ok();
                    }
                    let mut failed = false;
                    if let Some(stream) = &mut stream {
                        let mut bytes = [0; 16];
                        bytes[..8].copy_from_slice(&frame[0].to_le_bytes());
                        bytes[8..].copy_from_slice(&frame[1].to_le_bytes());
                        failed = stream.write_all(&bytes).is_err();
                    }
                    if failed {
                        stream = None;
                    }
                }
            },
        )
    } else {
        Worker::spawn(
            "Oscilloscope (void)",
            CHANNEL_CAPACITY,
            move |rx: Receiver<Monitor>, _: Sender<()>| for _ in rx {},
        )
    };

    let port = matches.get_one::<String>("port").unwrap();
    let address = format!("127.0.0.1:{port}");
    let listener = std::net::TcpListener::bind(address).unwrap();
    for msg in listener.incoming().filter_map(|stream| {
        stream.ok().and_then(|mut stream| {
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).ok()?;
            from_bytes::<Message, Error>(&bytes).ok()
        })
    }) {
        worker.sender().send(msg).unwrap();
    }

    drop(oscilloscope);
    Ok(())
}
