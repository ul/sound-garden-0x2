use audio_program::{Context, TextOp, compile_program};
use audio_vm::{CHANNELS, Frame, Program, Sample, VM};
use crossbeam_channel::{Receiver, Sender};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rtrb::RingBuffer;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use thread_worker::Worker;

mod audio;
mod midi;
mod record;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;
const OSCILLOSCOPE_POLL_MS: u64 = 10;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub midi: MidiInputSelection,
}

pub use midi::{MidiInputSelection, list_inputs as list_midi_inputs};

#[derive(Clone, Debug)]
pub struct Monitor {
    pub scope: Frame,
    pub patterns: Vec<(u64, Frame)>,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Serialize, Deserialize)]
pub enum Msg {
    Play(bool),
    Record(bool),
    LoadProgram(Vec<TextOp>),
    Monitor(u64),
    PatternMonitors(Vec<u64>),
    Oscilloscope(bool),
    Quit,
}

pub use Msg as Message;

pub fn run(rx: Receiver<Msg>, tx: Sender<Monitor>) {
    run_with_options(rx, tx, Options::default())
}

pub fn run_with_options(rx: Receiver<Msg>, tx: Sender<Monitor>, options: Options) {
    let vm = VM::new();
    let monitor = vm.monitor();
    let pattern_monitor = vm.pattern_monitor();
    let (producer, consumer) = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
    let (mut command_tx, command_rx) = RingBuffer::<audio::Command>::new(CHANNEL_CAPACITY);
    let (garbage_tx, mut garbage_rx) = RingBuffer::<Program>::new(CHANNEL_CAPACITY);
    let mut ctx = Context::default();
    let midi_frame = Arc::clone(&ctx.midi);
    let (midi_connection, midi_rx) = match midi::open_input(&options.midi) {
        Ok(Some((connection, consumer, name))) => {
            log::info!("Connected MIDI input: {name}");
            (Some(connection), Some(consumer))
        }
        Ok(None) => {
            if !matches!(options.midi, MidiInputSelection::None) {
                log::warn!("No MIDI input connected.");
            }
            (None, None)
        }
        Err(err) => {
            log::warn!("MIDI input unavailable: {err}");
            (None, None)
        }
    };

    std::thread::spawn(move || {
        loop {
            while let Ok(program) = garbage_rx.pop() {
                drop(program);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let player = Worker::spawn("Player", CHANNEL_CAPACITY, move |i, o| {
        audio::main(
            vm, producer, command_rx, garbage_tx, midi_rx, midi_frame, i, o,
        )
        .unwrap();
    });
    let sample_rate = player.receiver().recv().unwrap();

    let recorder = Worker::spawn("Recorder", CHANNEL_CAPACITY, move |i, o| {
        record::main(sample_rate, consumer, i, o).unwrap();
    });

    let scope = Worker::spawn(
        "Oscilloscope",
        1,
        move |rx: Receiver<bool>, _: Sender<()>| {
            let mut enabled = false;
            loop {
                if enabled {
                    crossbeam_channel::select! {
                        recv(rx) -> msg => match msg {
                            Ok(on) => enabled = on,
                            Err(_) => break,
                        },
                        default(Duration::from_millis(OSCILLOSCOPE_POLL_MS)) => {
                            let mut frame = [0.0; CHANNELS];
                            for (a, x) in monitor.iter().zip(&mut frame) {
                                *x = f64::from_bits(a.load(Ordering::Relaxed));
                            }
                            let patterns = pattern_monitor
                                .try_lock()
                                .map(|monitor| monitor.clone())
                                .unwrap_or_default();
                            if tx.send(Monitor { scope: frame, patterns }).is_err() { break; };
                        }
                    }
                } else {
                    match rx.recv() {
                        Ok(on) => enabled = on,
                        Err(_) => break,
                    }
                }
            }
        },
    );

    let _midi_connection = midi_connection;
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
            Msg::PatternMonitors(ids) => {
                command_tx.push(audio::Command::PatternMonitors(ids)).ok();
            }
            Msg::Oscilloscope(on) => {
                scope.sender().send(on).ok();
            }
            Msg::Quit => {
                break;
            }
        }
    }
}
