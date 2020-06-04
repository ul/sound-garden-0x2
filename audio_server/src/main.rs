use anyhow::Result;
use audio_server::{run, Message};
use audio_vm::Frame;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crossbeam_channel::{Receiver, Sender};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;

fn main() -> Result<()> {
    let matches = app_from_crate!()
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .default_value("31337")
                .help("Port to listen to for programs."),
        )
        .arg(
            Arg::with_name("scope-port")
                .short("o")
                .long("scope-port")
                .value_name("SCOPE_PORT")
                .help("Port to send oscilloscope samples."),
        )
        .get_matches();

    let scope_port = matches
        .value_of("scope-port")
        .and_then(|s| s.parse::<u16>().ok());

    let worker = Worker::spawn("Synth", CHANNEL_CAPACITY, run);

    let oscilloscope = if let Some(port) = scope_port {
        Worker::spawn(
            "Oscilloscope (pub)",
            CHANNEL_CAPACITY,
            move |rx: Receiver<Frame>, _: Sender<()>| {
                let socket = nng::Socket::new(nng::Protocol::Pub0).unwrap();
                let url = format!("tcp://127.0.0.1:{}", port);
                socket.dial_async(&url).unwrap();
                for frame in rx {
                    socket
                        .send(&[frame[0].to_le_bytes(), frame[1].to_le_bytes()].concat())
                        .ok();
                }
            },
        )
    } else {
        Worker::spawn(
            "Oscilloscope (void)",
            CHANNEL_CAPACITY,
            move |rx: Receiver<Frame>, _: Sender<()>| {
                for _ in rx {}
            },
        )
    };

    let port = matches.value_of("port").unwrap();
    let address = format!("127.0.0.1:{}", port);
    let listener = std::net::TcpListener::bind(address).unwrap();
    for msg in listener.incoming().filter_map(|stream| {
        stream
            .ok()
            .and_then(|stream| serde_json::from_reader::<_, Message>(stream).ok())
    }) {
        worker.sender().send(msg).unwrap();
    }

    drop(oscilloscope);

    Ok(())
}
