use anyhow::Result;
use audio_ops::pure::clip;
use audio_vm::{Sample, CHANNELS, VM};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use ringbuf::Producer;
use std::sync::{Arc, Mutex};

pub fn main(
    vm: Arc<Mutex<VM>>,
    producer: Producer<Sample>,
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
    let sample_rate = config.sample_rate().0;
    tx.send(sample_rate)?;

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, &config.into(), vm, producer, rx),
        cpal::SampleFormat::I16 => run::<i16>(&device, &config.into(), vm, producer, rx),
        cpal::SampleFormat::U16 => run::<u16>(&device, &config.into(), vm, producer, rx),
    }
}

fn run<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    vm: Arc<Mutex<VM>>,
    mut producer: Producer<Sample>,
    rx: Receiver<()>,
) -> Result<(), anyhow::Error>
where
    T: cpal::Sample,
{
    let channels = config.channels as usize;

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &vm, &mut producer)
        },
        err_fn,
    )?;
    stream.play()?;

    for _ in rx {}

    Ok(())
}

fn write_data<T>(
    output: &mut [T],
    channels: usize,
    vm: &Arc<Mutex<VM>>,
    producer: &mut Producer<Sample>,
) where
    T: cpal::Sample,
{
    let mut vm = vm.lock().unwrap();
    for frame in output.chunks_mut(channels) {
        for (sample, &value) in frame.iter_mut().zip(vm.next_frame().iter()) {
            let value = clip(value);
            *sample = cpal::Sample::from::<f32>(&(value as f32));
            producer.push(value).ok();
        }
    }
}
