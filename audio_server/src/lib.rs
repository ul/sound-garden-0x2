use audio_program::{Context, Diagnostic, TextOp, compile_program_with_diagnostics};
use audio_vm::{CHANNELS, Frame, Program, Sample, VM};
use crossbeam_channel::{Receiver, Sender};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rtrb::RingBuffer;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use thread_worker::Worker;

mod audio;
mod midi;
mod record;
mod telemetry;

pub use telemetry::Meters;

const CHANNEL_CAPACITY: usize = 64;
/// It's about 500ms, should be more than enough for write cycle of ~10ms.
const RECORD_BUFFER_CAPACITY: usize = 48000;
const OSCILLOSCOPE_POLL_MS: u64 = 10;
/// Per-sample scope capture between polls; ~340 ms at 48 kHz, far more than
/// one poll interval.
const SCOPE_RING_FRAMES: usize = 16384;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub midi: MidiInputSelection,
    /// Requested sample rate in Hz; None keeps the device default.
    pub sample_rate: Option<u32>,
    /// Requested audio buffer size in frames; None keeps the device default.
    pub buffer_frames: Option<u32>,
}

pub use audio::list_outputs as list_audio_outputs;
pub use midi::{MidiInputSelection, MidiMessage, list_inputs as list_midi_inputs};

#[derive(Clone, Debug)]
pub struct Monitor {
    pub scope: Frame,
    pub patterns: Vec<(u64, Frame)>,
    pub meters: Meters,
    /// Name of the connected MIDI input, if any.
    pub midi_device: Option<Arc<str>>,
    /// Warnings from the latest program compile.
    pub diagnostics: Arc<Diagnostics>,
    /// Every frame of the monitored node since the previous message.
    pub samples: Vec<Frame>,
}

/// Compile warnings for the latest program, each tied to the node that caused
/// it. `generation` changes with every compile.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub generation: u64,
    pub items: Vec<Diagnostic>,
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
    // Pattern monitor ids are updated from this thread, not the audio thread:
    // replacing them locks and allocates.
    let pattern_monitor_ids = vm.pattern_monitor();
    let (record_tx, record_rx) = RingBuffer::<Sample>::new(RECORD_BUFFER_CAPACITY);
    // Set by the recorder while it has a file open; the audio thread only feeds
    // the ring buffer while it is set.
    let recording = Arc::new(AtomicBool::new(false));
    let (mut command_tx, command_rx) = RingBuffer::<audio::Command>::new(CHANNEL_CAPACITY);
    let (garbage_tx, mut garbage_rx) = RingBuffer::<Program>::new(CHANNEL_CAPACITY);
    let mut ctx = Context::default();
    let midi_frame = Arc::clone(&ctx.midi);
    let midi_controls = Arc::clone(&ctx.midi_controls);
    let telemetry = Arc::new(telemetry::Telemetry::new());
    let (scope_tx, mut scope_rx) = RingBuffer::<Frame>::new(SCOPE_RING_FRAMES);
    // Written here after each compile, read by the monitor thread; neither is
    // the audio thread, so a mutex is fine.
    let diagnostics = Arc::new(Mutex::new(Arc::new(Diagnostics::default())));
    let monitor_diagnostics = Arc::clone(&diagnostics);
    let engine_telemetry = Arc::clone(&telemetry);
    let requested_sample_rate = options.sample_rate;
    let buffer_frames = options.buffer_frames;
    let (midi_connection, midi_rx, midi_device) =
        match midi::open_input(&options.midi, Arc::clone(&telemetry)) {
            Ok(Some((connection, consumer, name))) => {
                log::info!("Connected MIDI input: {name}");
                (
                    Some(connection),
                    Some(consumer),
                    Some(Arc::<str>::from(name)),
                )
            }
            Ok(None) => {
                if !matches!(options.midi, MidiInputSelection::None) {
                    log::warn!("No MIDI input connected.");
                }
                (None, None, None)
            }
            Err(err) => {
                log::warn!("MIDI input unavailable: {err}");
                (None, None, None)
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

    let recording_flag = Arc::clone(&recording);
    let player = Worker::spawn("Player", CHANNEL_CAPACITY, move |i, o| {
        let engine = audio::Engine {
            vm,
            command_rx,
            garbage_tx,
            record_tx,
            recording: Arc::clone(&recording_flag),
            midi_rx,
            midi_frame,
            midi_controls,
            telemetry: engine_telemetry,
            scope_tx,
        };
        audio::main(engine, requested_sample_rate, buffer_frames, i, o).unwrap();
    });
    let sample_rate = player.receiver().recv().unwrap();

    let recorder = Worker::spawn("Recorder", CHANNEL_CAPACITY, move |i, o| {
        record::main(sample_rate, record_rx, recording, i, o).unwrap();
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
                            let meters = telemetry.snapshot();
                            let midi_device = midi_device.clone();
                            let diagnostics = monitor_diagnostics
                                .lock()
                                .map(|diagnostics| Arc::clone(&diagnostics))
                                .unwrap_or_default();
                            let mut samples = Vec::with_capacity(scope_rx.slots());
                            while let Ok(sample) = scope_rx.pop() {
                                samples.push(sample);
                            }
                            if tx.send(Monitor { scope: frame, patterns, meters, midi_device, diagnostics, samples }).is_err() { break; };
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
                let (program, items) =
                    compile_program_with_diagnostics(&ops, sample_rate, &mut ctx);
                if let Ok(mut latest) = diagnostics.lock() {
                    *latest = Arc::new(Diagnostics {
                        generation: latest.generation + 1,
                        items,
                    });
                }
                command_tx.push(audio::Command::LoadProgram(program)).ok();
            }
            Msg::Monitor(id) => {
                command_tx.push(audio::Command::Monitor(id)).ok();
            }
            Msg::PatternMonitors(ids) => {
                audio_vm::set_pattern_monitor_ids(&pattern_monitor_ids, &ids);
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
