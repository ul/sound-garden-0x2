use anyhow::Result;
use audio_ops::{MAX_MIDI_EVENTS_PER_FRAME, MidiEvent, MidiFrameEvents, pure::clip};
use audio_vm::{CHANNELS, Program, Sample, VM};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use rtrb::{Consumer, Producer, PushError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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
    pub midi_rx: Option<Consumer<MidiEvent>>,
    pub midi_frame: Arc<MidiFrameEvents>,
}

pub fn main(engine: Engine, rx: Receiver<()>, tx: Sender<u32>) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(anyhow::anyhow!("No default device available."))?;
    let config = device.default_output_config()?;
    let channels = config.channels() as usize;
    if channels != CHANNELS {
        return Err(anyhow::anyhow!(
            "audio_vm supports exactly {} channels, but your device has {}.",
            CHANNELS,
            channels
        ));
    }

    let sample_rate = config.sample_rate();
    tx.send(sample_rate)?;

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, config.into(), engine, rx),
        cpal::SampleFormat::I16 => run::<i16>(&device, config.into(), engine, rx),
        cpal::SampleFormat::U16 => run::<u16>(&device, config.into(), engine, rx),
        sample_format => Err(anyhow::anyhow!(
            "Unsupported sample format: {sample_format:?}"
        )),
    }
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
    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    // Whether midi_frame still holds the previous frame's events and must be
    // cleared; lets frames without MIDI skip touching the shared slots.
    let mut midi_frame_dirty = false;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            engine.write_data(data, channels, &mut midi_frame_dirty)
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    for _ in rx {}
    Ok(())
}

impl Engine {
    fn write_data<T>(&mut self, output: &mut [T], channels: usize, midi_frame_dirty: &mut bool)
    where
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

        let mut midi_events = [MidiEvent::note_off(0, 0); MAX_MIDI_EVENTS_PER_FRAME];
        for frame in output.chunks_mut(channels) {
            if let Some(midi_rx) = self.midi_rx.as_mut() {
                let mut midi_count = 0;
                while midi_count < midi_events.len() {
                    let Ok(event) = midi_rx.pop() else {
                        break;
                    };
                    midi_events[midi_count] = event;
                    midi_count += 1;
                }
                if midi_count > 0 || *midi_frame_dirty {
                    self.midi_frame.set_events(&midi_events[..midi_count]);
                    *midi_frame_dirty = midi_count > 0;
                }
            }
            for (sample, &value) in frame.iter_mut().zip(self.vm.next_frame().iter()) {
                let value = clip(value);
                *sample = T::from_sample(value as f32);
                if record {
                    self.record_tx.push(value).ok();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_program::{Context, TextOp, compile_program};
    use rtrb::RingBuffer;

    struct Harness {
        engine: Engine,
        record_rx: Consumer<Sample>,
        midi_tx: rtrb::Producer<MidiEvent>,
        midi_frame_dirty: bool,
    }

    fn harness(record_capacity: usize) -> Harness {
        let mut vm = VM::new();
        vm.set_xfade_duration(0.0);
        let ops = [TextOp {
            id: 1,
            op: "0.5".to_owned(),
        }];
        vm.load_program(compile_program(&ops, 48_000, &mut Context::new()));
        vm.play();
        let (_command_tx, command_rx) = RingBuffer::new(4);
        let (garbage_tx, _garbage_rx) = RingBuffer::new(4);
        let (record_tx, record_rx) = RingBuffer::new(record_capacity);
        let (midi_tx, midi_rx) = RingBuffer::new(16);
        Harness {
            engine: Engine {
                vm,
                command_rx,
                garbage_tx,
                record_tx,
                recording: Arc::new(AtomicBool::new(false)),
                midi_rx: Some(midi_rx),
                midi_frame: Arc::new(MidiFrameEvents::new()),
            },
            record_rx,
            midi_tx,
            midi_frame_dirty: false,
        }
    }

    impl Harness {
        fn callback(&mut self, frames: usize) -> Vec<f32> {
            let mut output = vec![0.0f32; frames * CHANNELS];
            self.engine
                .write_data(&mut output, CHANNELS, &mut self.midi_frame_dirty);
            output
        }

        fn recorded(&mut self) -> usize {
            let mut count = 0;
            while self.record_rx.pop().is_ok() {
                count += 1;
            }
            count
        }

        fn midi_frame_len(&self) -> usize {
            let mut events = [MidiEvent::note_off(0, 0); MAX_MIDI_EVENTS_PER_FRAME];
            self.engine.midi_frame.copy_events(&mut events)
        }
    }

    #[test]
    fn records_only_while_recording_and_only_whole_callbacks() {
        let mut h = harness(64);
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
    fn midi_frame_is_written_for_events_and_cleared_once_after() {
        let mut h = harness(4);
        h.midi_tx.push(MidiEvent::note_on(0, 60, 1.0)).unwrap();
        // The event lands on the callback's first frame and is cleared on the
        // next one, so after a multi-frame callback the slots are empty.
        h.callback(1);
        assert_eq!(h.midi_frame_len(), 1);
        h.callback(1);
        assert_eq!(h.midi_frame_len(), 0);
        assert!(!h.midi_frame_dirty);
    }
}
