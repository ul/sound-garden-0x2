use crate::{
    midi::{MidiMessage, TimedMidiEvent},
    telemetry::{CallbackTiming, OutputLevels, Telemetry},
};
use anyhow::Result;
use audio_ops::{MAX_MIDI_EVENTS_PER_FRAME, MidiControls, MidiEvent, MidiFrameEvents, pure::clip};
use audio_vm::{CHANNELS, Frame, Program, Sample, VM};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use rtrb::{Consumer, Producer, PushError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub enum Command {
    Play(bool),
    LoadProgram(Program),
    Monitor(u64),
}

/// Everything the audio callback owns.
pub struct Engine {
    pub vm: VM,
    pub command_rx: Consumer<Command>,
    /// Replaced programs go back to a non-realtime thread to be dropped.
    pub garbage_tx: Producer<Program>,
    /// Output samples for the recorder, pushed only while `recording` is set.
    pub record_tx: Producer<Sample>,
    pub recording: Arc<AtomicBool>,
    pub midi_rx: Option<Consumer<TimedMidiEvent>>,
    /// Notes for the current frame, read by `mpoly`.
    pub midi_frame: Arc<MidiFrameEvents>,
    /// Controller and bend values, updated at each message's frame.
    pub midi_controls: Arc<MidiControls>,
    /// Load, dropouts and output levels for the GUI.
    pub telemetry: Arc<Telemetry>,
    /// Every frame of the monitored node, for the GUI's waveform, spectrum
    /// and value readout. Frames are dropped if the reader falls behind.
    pub scope_tx: Producer<Frame>,
}

/// State carried from one audio callback to the next.
#[derive(Default)]
struct CallbackState {
    /// Whether midi_frame still holds the previous frame's events and must be
    /// cleared; lets frames without MIDI skip touching the shared slots.
    midi_frame_dirty: bool,
    /// Start of the previous callback: MIDI that arrived since then is spread
    /// over the current buffer at the same relative position.
    previous_callback: Option<Instant>,
    timing: CallbackTiming,
}

/// `sample_rate` requests a device sample rate, falling back to the device
/// default if the device can't run at it; `buffer_frames` requests a device
/// buffer size, clamped to what the device supports. `None` keeps the device
/// default for either.
pub fn main(
    engine: Engine,
    sample_rate: Option<u32>,
    buffer_frames: Option<u32>,
    rx: Receiver<()>,
    tx: Sender<u32>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(anyhow::anyhow!("No default device available."))?;
    let config = output_config(&device, sample_rate)?;
    let mut stream_config = config.config();
    if let Some(frames) = buffer_frames {
        let frames = match config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                let clamped = frames.clamp(*min, *max);
                if clamped != frames {
                    log::warn!(
                        "Buffer size {frames} is outside the device's {min}..={max}; using {clamped}."
                    );
                }
                clamped
            }
            cpal::SupportedBufferSize::Unknown => frames,
        };
        stream_config.buffer_size = cpal::BufferSize::Fixed(frames);
    }
    let channels = config.channels() as usize;
    if channels != CHANNELS {
        return Err(anyhow::anyhow!(
            "audio_vm supports exactly {} channels, but your device has {}.",
            CHANNELS,
            channels
        ));
    }

    let sample_rate = config.sample_rate();
    engine.telemetry.set_sample_rate(sample_rate);
    tx.send(sample_rate)?;

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, stream_config, engine, rx),
        cpal::SampleFormat::I16 => run::<i16>(&device, stream_config, engine, rx),
        cpal::SampleFormat::U16 => run::<u16>(&device, stream_config, engine, rx),
        sample_format => Err(anyhow::anyhow!(
            "Unsupported sample format: {sample_format:?}"
        )),
    }
}

/// The device default config, or one at `sample_rate` if the device supports
/// it with our channel count, preferring the default sample format.
fn output_config(
    device: &cpal::Device,
    sample_rate: Option<u32>,
) -> Result<cpal::SupportedStreamConfig> {
    let default = device.default_output_config()?;
    let Some(rate) = sample_rate.filter(|&rate| rate != default.sample_rate()) else {
        return Ok(default);
    };
    let mut ranges = device
        .supported_output_configs()?
        .filter(|range| range.channels() as usize == CHANNELS && range.contains_rate(rate))
        .filter(|range| {
            matches!(
                range.sample_format(),
                cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
            )
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.sample_format() != default.sample_format());
    match ranges.into_iter().next() {
        Some(range) => Ok(range.with_sample_rate(rate)),
        None => {
            log::warn!(
                "Sample rate {rate} Hz isn't supported by the device; using {} Hz. See --list-audio.",
                default.sample_rate()
            );
            Ok(default)
        }
    }
}

/// Output devices of the default host and the configs each supports, one
/// line per device followed by indented config lines.
pub fn list_outputs() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok());
    let mut lines = Vec::new();
    for device in host.output_devices()? {
        let is_default = default_id.is_some() && device.id().ok() == default_id;
        lines.push(format!(
            "{}{}",
            device,
            if is_default { " (default, used)" } else { "" }
        ));
        if let Ok(config) = device.default_output_config() {
            lines.push(format!(
                "    default: {} ch, {}, {} Hz",
                config.channels(),
                config.sample_format(),
                config.sample_rate()
            ));
        }
        let Ok(ranges) = device.supported_output_configs() else {
            lines.push("    supported configs unavailable".to_string());
            continue;
        };
        // Devices often report one range per rate; group them by channels,
        // format and buffer size so each group is one line of rates.
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for range in ranges {
            let key = format!(
                "{} ch, {}, buffer {}",
                range.channels(),
                range.sample_format(),
                match range.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => format!("{min}..={max}"),
                    cpal::SupportedBufferSize::Unknown => "unknown".to_string(),
                }
            );
            let rates = if range.min_sample_rate() == range.max_sample_rate() {
                range.min_sample_rate().to_string()
            } else {
                format!("{}..={}", range.min_sample_rate(), range.max_sample_rate())
            };
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, group)) => {
                    if !group.contains(&rates) {
                        group.push(rates)
                    }
                }
                None => groups.push((key, vec![rates])),
            }
        }
        for (key, rates) in groups {
            lines.push(format!("    {key}: {} Hz", rates.join(", ")));
        }
    }
    Ok(lines)
}

fn run<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut engine: Engine,
    rx: Receiver<()>,
) -> Result<()>
where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate;
    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    let mut state = CallbackState::default();
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let start = Instant::now();
            engine.write_data(data, channels, &mut state, start);
            let frames = data.len() / channels.max(1);
            let (load, dropout) = state
                .timing
                .record(start, Instant::now(), frames, sample_rate);
            engine
                .telemetry
                .record_callback(frames as u32, load, dropout);
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    for _ in rx {}
    Ok(())
}

impl Engine {
    /// `now` is when this callback started; it is a parameter so tests can
    /// drive the clock.
    fn write_data<T>(
        &mut self,
        output: &mut [T],
        channels: usize,
        state: &mut CallbackState,
        now: Instant,
    ) where
        T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
    {
        audio_vm::enable_flush_to_zero();
        while let Ok(command) = self.command_rx.pop() {
            match command {
                Command::Play(true) => self.vm.play(),
                Command::Play(false) => self.vm.pause(),
                Command::LoadProgram(program) => {
                    let garbage = self.vm.load_program(program);
                    if let Err(PushError::Full(garbage)) = self.garbage_tx.push(garbage) {
                        // Avoid deallocating the old program in the audio callback.
                        std::mem::forget(garbage);
                    }
                }
                Command::Monitor(id) => self.vm.set_monitor_id(id),
            }
        }

        // Decide once per callback, and only if the whole callback fits, so the
        // recorder never sees a partial frame (which would swap its channels).
        let record =
            self.recording.load(Ordering::Acquire) && self.record_tx.slots() >= output.len();

        // MIDI that arrived during the previous callback period is placed at
        // the same relative position within this buffer: a constant one-period
        // delay instead of up to a period of jitter from bunching every event
        // onto the first frame.
        let frames = output.len() / channels.max(1);
        let window = state
            .previous_callback
            .map(|start| (start, now.saturating_duration_since(start)));
        state.previous_callback = Some(now);
        let target_frame = |at: Instant| match window {
            Some((start, period)) if !period.is_zero() && at > start => {
                let position = (at - start).as_secs_f64() / period.as_secs_f64();
                ((position * frames as f64) as usize).min(frames.saturating_sub(1))
            }
            // First callback, or events left over from earlier periods.
            _ => 0,
        };

        let mut levels = OutputLevels::default();
        let mut midi_events = [MidiEvent::note_off(0, 0); MAX_MIDI_EVENTS_PER_FRAME];
        for (index, frame) in output.chunks_mut(channels).enumerate() {
            if let Some(midi_rx) = self.midi_rx.as_mut() {
                let mut midi_count = 0;
                while let Ok(&timed) = midi_rx.peek() {
                    // Messages are in arrival order; stop at the first one that
                    // belongs to a later frame or arrived during this callback.
                    if timed.at >= now || target_frame(timed.at) > index {
                        break;
                    }
                    match timed.message {
                        MidiMessage::Note(event) => {
                            if midi_count == midi_events.len() {
                                // This frame's note slots are full; the rest
                                // wait for the next frame, in order.
                                break;
                            }
                            midi_events[midi_count] = event;
                            midi_count += 1;
                        }
                        MidiMessage::Controller { controller, value } => {
                            self.midi_controls.set_controller(controller, value)
                        }
                        MidiMessage::Bend(value) => self.midi_controls.set_bend(value),
                    }
                    midi_rx.pop().ok();
                }
                if midi_count > 0 || state.midi_frame_dirty {
                    self.midi_frame.set_events(&midi_events[..midi_count]);
                    state.midi_frame_dirty = midi_count > 0;
                }
            }
            let vm_frame = self.vm.next_frame();
            levels.add(&vm_frame);
            self.scope_tx.push(self.vm.scope()).ok();
            for (sample, &value) in frame.iter_mut().zip(vm_frame.iter()) {
                let value = clip(value);
                *sample = T::from_sample(value as f32);
                if record {
                    self.record_tx.push(value).ok();
                }
            }
        }
        self.telemetry.record_output(&levels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_program::{Context, TextOp, compile_program};
    use rtrb::RingBuffer;
    use std::time::Duration;

    const PERIOD: Duration = Duration::from_millis(10);

    struct Harness {
        engine: Engine,
        state: CallbackState,
        record_rx: Consumer<Sample>,
        midi_tx: Producer<TimedMidiEvent>,
        scope_rx: Consumer<Frame>,
        /// Start of the next callback.
        clock: Instant,
    }

    fn harness(source: &str, record_capacity: usize) -> Harness {
        let mut ctx = Context::new();
        let ops = source
            .split_whitespace()
            .enumerate()
            .map(|(index, op)| TextOp {
                id: index as u64 + 1,
                op: op.to_owned(),
            })
            .collect::<Vec<_>>();
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        vm.set_declick_duration(0.0);
        vm.load_program(compile_program(&ops, 48_000, &mut ctx));
        vm.play();
        let (_command_tx, command_rx) = RingBuffer::new(4);
        let (garbage_tx, _garbage_rx) = RingBuffer::new(4);
        let (record_tx, record_rx) = RingBuffer::new(record_capacity);
        let (midi_tx, midi_rx) = RingBuffer::new(16);
        let (scope_tx, scope_rx) = RingBuffer::new(64);
        Harness {
            engine: Engine {
                vm,
                command_rx,
                garbage_tx,
                record_tx,
                recording: Arc::new(AtomicBool::new(false)),
                midi_rx: Some(midi_rx),
                midi_frame: Arc::clone(&ctx.midi),
                midi_controls: Arc::clone(&ctx.midi_controls),
                telemetry: Arc::new(Telemetry::new()),
                scope_tx,
            },
            scope_rx,
            state: CallbackState::default(),
            record_rx,
            midi_tx,
            clock: Instant::now(),
        }
    }

    impl Harness {
        /// Run one callback of `frames` frames; callbacks are PERIOD apart.
        fn callback(&mut self, frames: usize) -> Vec<f32> {
            let mut output = vec![0.0f32; frames * CHANNELS];
            self.engine
                .write_data(&mut output, CHANNELS, &mut self.state, self.clock);
            self.clock += PERIOD;
            output
        }

        /// Left channel of one callback.
        fn left(&mut self, frames: usize) -> Vec<f32> {
            self.callback(frames)
                .into_iter()
                .step_by(CHANNELS)
                .collect()
        }

        /// Queue an event that arrived `fraction` of a period after the
        /// previous callback started.
        fn midi_at(&mut self, fraction: f64, event: MidiEvent) {
            self.message_at(fraction, MidiMessage::Note(event));
        }

        fn message_at(&mut self, fraction: f64, message: MidiMessage) {
            let at = self.clock - PERIOD + PERIOD.mul_f64(fraction);
            self.midi_tx.push(TimedMidiEvent { at, message }).unwrap();
        }

        fn recorded(&mut self) -> usize {
            let mut count = 0;
            while self.record_rx.pop().is_ok() {
                count += 1;
            }
            count
        }
    }

    /// A voice body that outputs its gate, so the output shows which frame
    /// each note-on/off landed on.
    const GATE_PROBE: &str = "[ swap pop ] mpoly:1";

    #[test]
    fn records_only_while_recording_and_only_whole_callbacks() {
        let mut h = harness("0.5", 64);
        assert_eq!(h.callback(8), [0.5; 16]);
        assert_eq!(h.recorded(), 0);

        h.engine.recording.store(true, Ordering::Release);
        h.callback(8);
        assert_eq!(h.recorded(), 16);

        // 40 samples don't fit in 64 - 32 free slots: skip the whole callback
        // rather than record a partial frame.
        h.callback(16);
        h.callback(20);
        assert_eq!(h.recorded(), 32);
    }

    #[test]
    fn midi_lands_at_its_relative_position_one_period_later() {
        let mut h = harness(GATE_PROBE, 4);
        h.callback(8);
        h.midi_at(0.25, MidiEvent::note_on(0, 60, 1.0));
        h.midi_at(0.75, MidiEvent::note_off(0, 60));
        assert_eq!(h.left(8), [0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn midi_arriving_during_a_callback_waits_for_the_next_one() {
        let mut h = harness(GATE_PROBE, 4);
        h.callback(8);
        // Arrives exactly when the next callback starts: not before `now`.
        h.midi_at(1.0, MidiEvent::note_on(0, 60, 1.0));
        assert_eq!(h.left(8), [0.0; 8]);
        // One period later it is at the very start of the buffer.
        assert_eq!(h.left(8), [1.0; 8]);
    }

    #[test]
    fn first_callback_plays_pending_midi_immediately() {
        let mut h = harness(GATE_PROBE, 4);
        h.midi_at(0.5, MidiEvent::note_on(0, 60, 1.0));
        assert_eq!(h.left(4), [1.0; 4]);
    }

    #[test]
    fn midi_frame_is_cleared_once_after_events() {
        let mut h = harness(GATE_PROBE, 4);
        h.midi_at(0.0, MidiEvent::note_on(0, 60, 1.0));
        h.callback(1);
        assert!(h.state.midi_frame_dirty);
        h.callback(1);
        assert!(!h.state.midi_frame_dirty);
        let mut events = [MidiEvent::note_off(0, 0); MAX_MIDI_EVENTS_PER_FRAME];
        assert_eq!(h.engine.midi_frame.copy_events(&mut events), 0);
    }

    #[test]
    fn knob_and_bend_change_on_the_frame_they_arrived_at() {
        let mut h = harness("cc':74:0.25 bend' +", 4);
        assert_eq!(h.left(4), [0.25; 4], "default before the knob moves");
        h.message_at(
            0.5,
            MidiMessage::Controller {
                controller: 74,
                value: 0.75,
            },
        );
        h.message_at(0.75, MidiMessage::Bend(-0.5));
        assert_eq!(h.left(4), [0.25, 0.25, 0.75, 0.25]);
    }

    #[test]
    fn controller_values_survive_a_program_reload() {
        let mut h = harness("cc':1", 4);
        h.callback(4);
        h.message_at(
            0.0,
            MidiMessage::Controller {
                controller: 1,
                value: 0.5,
            },
        );
        assert_eq!(h.left(2), [0.5; 2]);

        // Commit a different program reading the same knob: it starts where
        // the knob is, not at its default. (The harness compiles with its own
        // context; share the controller store like audio_server does.)
        let mut ctx = Context::new();
        ctx.midi_controls = Arc::clone(&h.engine.midi_controls);
        let ops = [TextOp {
            id: 99,
            op: "cc':1:0.1".to_owned(),
        }];
        h.engine
            .vm
            .load_program(compile_program(&ops, 48_000, &mut ctx));
        assert_eq!(h.left(2), [0.5; 2]);
    }

    #[test]
    fn output_levels_are_measured_before_the_clip() {
        // 1.5 is clipped to 1.0 on the way out, but the meter must show it.
        let mut h = harness("1.5 -0.5 +", 4);
        assert_eq!(h.left(4), [1.0; 4]);
        let meters = h.engine.telemetry.snapshot();
        assert_eq!(meters.peak, [1.0, 1.0]);
        let mut h = harness("1.5", 4);
        h.callback(4);
        let meters = h.engine.telemetry.snapshot();
        assert_eq!(meters.peak, [1.5, 1.5]);
        assert_eq!(meters.clipped, 8, "4 frames x 2 channels over 1.0");
        assert!((meters.rms[0] - 1.5).abs() < 1e-12);
    }

    #[test]
    fn every_frame_of_the_monitored_node_is_streamed() {
        // Constants get folded away; a MIDI control stays a statement of its own.
        let mut h = harness("cc':1:0.5 0.25 +", 4);
        h.engine.vm.set_monitor_id(1);
        h.callback(5);
        let mut samples = Vec::new();
        while let Ok(frame) = h.scope_rx.pop() {
            samples.push(frame);
        }
        assert_eq!(samples, [[0.5; 2]; 5], "node 1 reads 0.5");
    }
}
