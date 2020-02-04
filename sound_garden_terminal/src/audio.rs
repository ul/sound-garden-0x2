use anyhow::Result;
use audio_vm::{Sample, CHANNELS, VM};
use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use ringbuf::Producer;
use std::sync::{Arc, Mutex};

pub fn main(
    vm: Arc<Mutex<VM>>,
    mut producer: Producer<Sample>,
    rx: Receiver<()>,
    tx: Sender<u32>,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(anyhow::anyhow!("No default device available."))?;
    let format = device
        .default_output_format()
        .map_err(|_| anyhow::anyhow!("Default format error."))?;

    let channels = format.channels as usize;
    if channels != CHANNELS {
        return Err(anyhow::anyhow!(
            "audio_vm supports exactly {} channels, but your device has {}.",
            CHANNELS,
            channels
        ));
    }
    let sample_rate = format.sample_rate.0;
    tx.send(sample_rate)?;

    let event_loop = host.event_loop();
    let stream_id = event_loop
        .build_output_stream(&device, &format)
        .map_err(|_| anyhow::anyhow!("Failed to build output stream."))?;
    event_loop
        .play_stream(stream_id.clone())
        .map_err(|_| anyhow::anyhow!("Failed to play output stream."))?;

    event_loop.run(move |id, result| {
        match rx.try_recv() {
            Ok(_) => {}
            Err(TryRecvError::Disconnected) => {
                // cpal doesn't provide a civilized way to stop event loop.
                std::thread::sleep(std::time::Duration::from_secs(1));
                std::process::exit(0);
            }
            Err(TryRecvError::Empty) => {}
        }
        let data = match result {
            Ok(data) => data,
            Err(err) => {
                eprintln!("An error occurred on stream {:?}: {}.", id, err);
                return;
            }
        };
        let mut vm = vm.lock().unwrap();
        match data {
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::U16(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    let next_frame = vm.next_frame();
                    for (out, &sample) in frame.iter_mut().zip(&next_frame) {
                        let sample = clip(sample);
                        *out = ((sample * 0.5 + 0.5) * std::u16::MAX as Sample) as u16;
                        producer.push(sample).ok();
                    }
                }
            }
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::I16(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    let next_frame = vm.next_frame();
                    for (out, &sample) in frame.iter_mut().zip(&next_frame) {
                        let sample = clip(sample);
                        *out = (sample * std::i16::MAX as Sample) as i16;
                        producer.push(sample).ok();
                    }
                }
            }
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    let next_frame = vm.next_frame();
                    for (out, &sample) in frame.iter_mut().zip(&next_frame) {
                        let sample = clip(sample);
                        *out = sample as f32;
                        producer.push(sample).ok();
                    }
                }
            }
            _ => (),
        }
    });
}

fn clip(sample: Sample) -> Sample {
    if sample < -1.0 {
        -1.0
    } else if 1.0 < sample {
        1.0
    } else {
        sample
    }
}
