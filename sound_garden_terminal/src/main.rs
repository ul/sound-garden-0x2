mod app;
mod event;
mod ui;

use anyhow::Result;
use audio_program::TextOp;
use audio_server::Message;
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use crossbeam_channel::{Receiver, Sender};
use thread_worker::Worker;

pub fn main() -> Result<()> {
    let matches = app_from_crate!()
        .arg(
            Arg::with_name("FILENAME")
                .required(true)
                .index(1)
                .help("Path to the tree"),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .default_value("31337")
                .help("Port to send programs to"),
        )
        .get_matches();

    let filename = matches.value_of("FILENAME").unwrap().to_owned();
    let port = matches.value_of("port").unwrap();
    let address = format!("127.0.0.1:{}", port);

    let program_loader = {
        let address = address.to_owned();
        Worker::spawn(
            "Program loader",
            1,
            move |rx: Receiver<Vec<TextOp>>, _: Sender<()>| {
                for msg in rx {
                    if let Ok(stream) = std::net::TcpStream::connect(&address) {
                        serde_json::to_writer(stream, &Message::LoadProgram(msg)).ok();
                    }
                }
            },
        )
    };

    let player = {
        let address = address.to_owned();
        Worker::spawn("Player", 1, move |rx, _: Sender<()>| {
            for msg in rx {
                if let Ok(stream) = std::net::TcpStream::connect(&address) {
                    serde_json::to_writer(stream, &Message::Play(msg)).ok();
                }
            }
        })
    };

    let recorder = {
        let address = address.to_owned();
        Worker::spawn("Recorder", 1, move |rx, _: Sender<()>| {
            for msg in rx {
                if let Ok(stream) = std::net::TcpStream::connect(&address) {
                    serde_json::to_writer(stream, &Message::Record(msg)).ok();
                }
            }
        })
    };

    ui::main(
        program_loader.sender().clone(),
        player.sender().clone(),
        0,
        &filename,
        recorder.sender(),
    )?;

    Ok(())
}
