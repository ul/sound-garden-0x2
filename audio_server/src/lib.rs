use audio_program::{Context, TextOp, compile_program};
use audio_vm::{CHANNELS, Frame, Program, Sample, VM};
use crossbeam_channel::{Receiver, Sender};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rtrb::RingBuffer;
use serde::{Deserialize, Serialize};
use std::{sync::atomic::Ordering, time::Duration};
use thread_worker::Worker;

mod audio;
mod record;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;
const OSCILLOSCOPE_POLL_MS: u64 = 10;

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub enum Msg {
    Play(bool),
    Record(bool),
    LoadProgram(Vec<TextOp>),
    Monitor(u64),
    Quit,
}

pub use Msg as Message;

pub fn run(rx: Receiver<Msg>, tx: Sender<Frame>) {
    let vm = VM::new();
    let monitor = vm.monitor();
    let (producer, consumer) = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
    let (mut command_tx, command_rx) = RingBuffer::<audio::Command>::new(CHANNEL_CAPACITY);
    let (garbage_tx, mut garbage_rx) = RingBuffer::<Program>::new(CHANNEL_CAPACITY);

    std::thread::spawn(move || {
        loop {
            while let Ok(program) = garbage_rx.pop() {
                drop(program);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let player = Worker::spawn("Player", CHANNEL_CAPACITY, move |i, o| {
        audio::main(vm, producer, command_rx, garbage_tx, i, o).unwrap();
    });
    let sample_rate = player.receiver().recv().unwrap();

    let recorder = Worker::spawn("Recorder", CHANNEL_CAPACITY, move |i, o| {
        record::main(sample_rate, consumer, i, o).unwrap();
    });

    let _scope = Worker::spawn("Oscilloscope", 0, move |rx: Receiver<()>, _: Sender<()>| {
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
            Msg::Play(x) => {
                command_tx.push(audio::Command::Play(x)).ok();
            }
            Msg::Record(x) => {
                recorder.sender().send(x).ok();
            }
            Msg::LoadProgram(ops) => {
                let program = compile_program(&ops, sample_rate, &mut ctx);
                command_tx.push(audio::Command::LoadProgram(program)).ok();
            }
            Msg::Monitor(id) => {
                command_tx.push(audio::Command::Monitor(id)).ok();
            }
            Msg::Quit => {
                break;
            }
        }
    }
}
