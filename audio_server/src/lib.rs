use audio_program::{compile_program, Context, TextOp};
use audio_vm::{Frame, Sample, CHANNELS, VM};
use crossbeam_channel::{Receiver, Sender};
use ringbuf::RingBuffer;
use serde::{Deserialize, Serialize};
use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};
use thread_worker::Worker;

mod audio;
mod record;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;
const OSCILLOSCOPE_POLL_MS: u64 = 10;

#[derive(Serialize, Deserialize)]
pub enum Message {
    Play(bool),
    Record(bool),
    LoadProgram(Vec<TextOp>),
    Monitor(u64),
}

pub fn run(rx: Receiver<Message>, tx: Sender<Frame>) {
    let vm = Arc::new(Mutex::new(VM::new()));

    let rb = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
    let (producer, consumer) = rb.split();

    let player = {
        let vm = Arc::clone(&vm);
        Worker::spawn("Player", CHANNEL_CAPACITY, move |_: Receiver<()>, o| {
            audio::main(vm, producer, o).unwrap();
        })
    };

    let sample_rate = player.receiver().recv().unwrap();

    let recorder = {
        Worker::spawn("Recorder", CHANNEL_CAPACITY, move |i, o| {
            record::main(sample_rate, consumer, i, o).unwrap();
        })
    };

    let monitor = vm.lock().unwrap().monitor();
    let oscilloscope = Worker::spawn("Oscilloscope", 0, move |rx: Receiver<()>, _: Sender<()>| {
        loop {
            crossbeam_channel::select! {
                recv(rx) -> msg => if msg.is_err() { break; },
                default(Duration::from_millis(OSCILLOSCOPE_POLL_MS)) => {
                    let mut frame = [0.0; CHANNELS];
                    for (a, x) in monitor.iter().zip(&mut frame) {
                        *x = f64::from_bits(a.load(Ordering::Relaxed));
                    }
                    if tx.send(frame).is_err() { break; };
                }
            }
        }
    });

    let mut ctx = Context::default();

    for msg in rx {
        match msg {
            Message::Play(x) => {
                if x {
                    vm.lock().unwrap().play();
                } else {
                    vm.lock().unwrap().pause();
                }
            }
            Message::Record(x) => {
                recorder.sender().send(x).ok();
            }
            Message::LoadProgram(ops) => {
                let program = compile_program(&ops, sample_rate, &mut ctx);
                let garbage = {
                    vm.lock().unwrap().load_program(program);
                };
                drop(garbage);
            }
            Message::Monitor(id) => {
                vm.lock().unwrap().set_monitor_id(id);
            }
        }
    }

    drop(oscilloscope);
}
