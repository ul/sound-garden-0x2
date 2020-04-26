use anyhow::Result;
use audio_program::{compile_program, Context};
use audio_vm::{Sample, VM};
use clap::{crate_version, App, Arg};
use crossbeam_channel::Receiver;
use ringbuf::RingBuffer;
use std::sync::{Arc, Mutex};
use thread_worker::Worker;

mod audio;
mod lib;
mod record;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;

fn main() -> Result<()> {
    let matches = App::new("kak-lsp")
        .version(crate_version!())
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Sound Garden Audio Synth Server")
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .default_value("31337")
                .help("Port to listen to for programs."),
        )
        .get_matches();

    let vm = Arc::new(Mutex::new(VM::new()));
    {
        vm.lock().unwrap().stop();
    }

    let rb = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
    let (producer, consumer) = rb.split();

    let player = {
        let vm = Arc::clone(&vm);
        Worker::spawn("Player", CHANNEL_CAPACITY, move |_: Receiver<()>, o| {
            audio::main(vm, producer, o).unwrap();
        })
    };

    let sample_rate = player.receiver().recv()?;

    let recorder = {
        Worker::spawn("Recorder", CHANNEL_CAPACITY, move |i, o| {
            record::main(sample_rate, consumer, i, o).unwrap();
        })
    };

    let mut ctx = Context::default();

    let port = matches.value_of("port").unwrap();
    let address = format!("127.0.0.1:{}", port);
    let listener = std::net::TcpListener::bind(address).unwrap();
    for msg in listener.incoming().filter_map(|stream| {
        stream
            .ok()
            .and_then(|stream| serde_json::from_reader::<_, lib::Message>(stream).ok())
    }) {
        use lib::Message::{LoadProgram, Play, Record};
        match msg {
            Play(x) => {
                if x {
                    vm.lock().unwrap().play();
                } else {
                    vm.lock().unwrap().pause();
                }
            }
            Record(x) => {
                recorder.sender().send(x).ok();
            }
            LoadProgram(ops) => {
                let program = compile_program(&ops, sample_rate, &mut ctx);
                let garbage = {
                    vm.lock().unwrap().load_program(program);
                };
                drop(garbage);
            }
        }
    }

    Ok(())
}
