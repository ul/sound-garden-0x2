use anyhow::Result;
use audio_ops::{MAX_MIDI_EVENTS_PER_FRAME, MidiEvent, MidiFrameEvents, pure::clip};
use audio_vm::{CHANNELS, Program, Sample, VM};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use rtrb::{Consumer, Producer, PushError};
use std::sync::Arc;

pub enum Command {
    Play(bool),
    LoadProgram(Program),
    Monitor(u64),
    PatternMonitors(Vec<u64>),
}

pub fn main(
    vm: VM,
    producer: Producer<Sample>,
    command_rx: Consumer<Command>,
    garbage_tx: Producer<Program>,
    midi_rx: Option<Consumer<MidiEvent>>,
    midi_frame: Arc<MidiFrameEvents>,
    rx: Receiver<()>,
    tx: Sender<u32>,
) -> Result<()> {
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
        cpal::SampleFormat::F32 => run::<f32>(
            &device,
            config.into(),
            vm,
            producer,
            command_rx,
            garbage_tx,
            midi_rx,
            midi_frame,
            rx,
        ),
        cpal::SampleFormat::I16 => run::<i16>(
            &device,
            config.into(),
            vm,
            producer,
            command_rx,
            garbage_tx,
            midi_rx,
            midi_frame,
            rx,
        ),
        cpal::SampleFormat::U16 => run::<u16>(
            &device,
            config.into(),
            vm,
            producer,
            command_rx,
            garbage_tx,
            midi_rx,
            midi_frame,
            rx,
        ),
        sample_format => Err(anyhow::anyhow!(
            "Unsupported sample format: {sample_format:?}"
        )),
    }
}

fn run<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut vm: VM,
    mut producer: Producer<Sample>,
    mut command_rx: Consumer<Command>,
    mut garbage_tx: Producer<Program>,
    mut midi_rx: Option<Consumer<MidiEvent>>,
    midi_frame: Arc<MidiFrameEvents>,
    rx: Receiver<()>,
) -> Result<()>
where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(
                data,
                channels,
                &mut vm,
                &mut producer,
                &mut command_rx,
                &mut garbage_tx,
                midi_rx.as_mut(),
                &midi_frame,
            )
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    for _ in rx {}
    Ok(())
}

fn write_data<T>(
    output: &mut [T],
    channels: usize,
    vm: &mut VM,
    producer: &mut Producer<Sample>,
    command_rx: &mut Consumer<Command>,
    garbage_tx: &mut Producer<Program>,
    mut midi_rx: Option<&mut Consumer<MidiEvent>>,
    midi_frame: &MidiFrameEvents,
) where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    audio_vm::enable_flush_to_zero();
    while let Ok(command) = command_rx.pop() {
        match command {
            Command::Play(true) => vm.play(),
            Command::Play(false) => vm.pause(),
            Command::LoadProgram(program) => {
                let garbage = vm.load_program(program);
                if let Err(PushError::Full(garbage)) = garbage_tx.push(garbage) {
                    // Avoid deallocating the old program in the audio callback.
                    std::mem::forget(garbage);
                }
            }
            Command::Monitor(id) => vm.set_monitor_id(id),
            Command::PatternMonitors(ids) => vm.set_pattern_monitor_ids(ids),
        }
    }

    let mut midi_events = [MidiEvent::note_off(0, 0); MAX_MIDI_EVENTS_PER_FRAME];
    for frame in output.chunks_mut(channels) {
        let mut midi_count = 0;
        if let Some(midi_rx) = midi_rx.as_deref_mut() {
            while midi_count < midi_events.len() {
                let Ok(event) = midi_rx.pop() else {
                    break;
                };
                midi_events[midi_count] = event;
                midi_count += 1;
            }
        }
        midi_frame.set_events(&midi_events[..midi_count]);
        for (sample, &value) in frame.iter_mut().zip(vm.next_frame().iter()) {
            let value = clip(value);
            *sample = T::from_sample(value as f32);
            producer.push(value).ok();
        }
    }
}
