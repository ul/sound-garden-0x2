use anyhow::Result;
use audio_server::{Message, run};
use audio_vm::Frame;
use clap::{Arg, Command, crate_authors, crate_description, crate_name, crate_version};
use crossbeam_channel::{Receiver, Sender};
use rkyv::{from_bytes, rancor::Error};
use std::io::{Read, Write};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;

fn main() -> Result<()> {
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
        .get_matches();

    let scope_port = matches
        .get_one::<String>("scope-port")
        .and_then(|s| s.parse::<u16>().ok());
    let worker = Worker::spawn("Synth", CHANNEL_CAPACITY, run);

    let oscilloscope = if let Some(port) = scope_port {
        Worker::spawn(
            "Oscilloscope (tcp)",
            CHANNEL_CAPACITY,
            move |rx: Receiver<Frame>, _: Sender<()>| {
                let address = format!("127.0.0.1:{port}");
                let mut stream = std::net::TcpStream::connect(&address).ok();
                for frame in rx {
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
            move |rx: Receiver<Frame>, _: Sender<()>| for _ in rx {},
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
