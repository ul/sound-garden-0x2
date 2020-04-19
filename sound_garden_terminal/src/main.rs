mod app;
mod audio;
mod event;
mod record;
mod ui;

use anyhow::{anyhow, Result};
use audio_program::{compile_program, Context, TextOp};
use audio_vm::{Sample, VM};
use crossbeam_channel::{Receiver, Sender};
use ringbuf::RingBuffer;
use std::sync::{Arc, Mutex};
use thread_worker::Worker;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;

pub fn main() -> Result<()> {
    let filename = std::env::args().skip(1).next();

    if filename.is_none() {
        return Err(anyhow!("Filename is required."));
    }

    let filename = filename.unwrap();

    if let Some(port) = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        let address = format!("127.0.0.1:{}", port);
        let ops_wrk = Worker::spawn(
            "Program loader",
            1,
            move |rx: Receiver<Vec<TextOp>>, _: Sender<()>| {
                for msg in rx {
                    if let Ok(stream) = std::net::TcpStream::connect(address.clone()) {
                        serde_json::to_writer(stream, &msg).ok();
                    }
                }
            },
        );
        let play_wrk = Worker::spawn("Play", 0, |rx, _: Sender<()>| for _ in rx {});
        let record_wrk = Worker::spawn("Record", 0, |rx, _: Sender<()>| for _ in rx {});
        ui::main(
            ops_wrk.sender().clone(),
            play_wrk.sender().clone(),
            0,
            &filename,
            record_wrk.sender(),
        )?;
    } else {
        let vm = Arc::new(Mutex::new(VM::new()));
        {
            vm.lock().unwrap().stop();
        }
        let rb = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
        let (producer, consumer) = rb.split();

        let audio_wrk = {
            let vm = Arc::clone(&vm);
            Worker::spawn("Audio", CHANNEL_CAPACITY, move |i, o| {
                audio::main(vm, producer, i, o).unwrap();
            })
        };

        let sample_rate = audio_wrk.receiver().recv()?;

        let record_wrk = {
            let filename = filename.clone();
            Worker::spawn("Record", CHANNEL_CAPACITY, move |i, o| {
                record::main(&filename, sample_rate, consumer, i, o).unwrap();
            })
        };

        let ops_wrk = {
            let vm = Arc::clone(&vm);
            Worker::spawn(
                "Program loader",
                1,
                move |rx: Receiver<Vec<TextOp>>, _: Sender<()>| {
                    let mut ctx = Context::default();
                    for msg in rx {
                        let program = compile_program(&msg, sample_rate, &mut ctx);
                        let garbage = { vm.lock().unwrap().load_program(program) };
                        drop(garbage);
                    }
                },
            )
        };

        let play_wrk = {
            let vm = Arc::clone(&vm);
            Worker::spawn("Play", 1, move |rx: Receiver<bool>, _: Sender<()>| {
                for msg in rx {
                    if msg {
                        vm.lock().unwrap().play();
                    } else {
                        vm.lock().unwrap().pause();
                    }
                }
            })
        };

        ui::main(
            ops_wrk.sender().clone(),
            play_wrk.sender().clone(),
            sample_rate,
            &filename,
            record_wrk.sender(),
        )?;

        drop(play_wrk);
        drop(ops_wrk);
        drop(record_wrk);
        drop(audio_wrk);
    }

    Ok(())
}
