use anyhow::Result;
use audio_server::{run, Message};
use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
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
        .get_matches();

    let worker = Worker::spawn("Synth", CHANNEL_CAPACITY, run);

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

    Ok(())
}
