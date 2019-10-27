use crate::world::World;
use anyhow::Result;
use audio_program::parse_program;
use audio_vm::{Sample, CHANNELS, VM};
use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

pub fn main(rx: Receiver<World>, _tx: Sender<()>) -> Result<()> {
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
    let mut vm = VM::new();

    let event_loop = host.event_loop();
    let stream_id = event_loop
        .build_output_stream(&device, &format)
        .map_err(|_| anyhow::anyhow!("Failed to build output stream."))?;
    event_loop
        .play_stream(stream_id.clone())
        .map_err(|_| anyhow::anyhow!("Failed to play output stream."))?;

    event_loop.run(move |id, result| {
        match rx.try_recv() {
            Ok(_) => {
                // Just for the basic test, not going to re-parse program for each change inside audio loop.
                vm.load_program(parse_program(&"1 w 440 * s", sample_rate));
            }
            Err(TryRecvError::Disconnected) => {
                // cpal doesn't provide a civilized way to stop event loop.
                info!("Audio: Don't wait for me, gonna nuke entire process.");
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
        match data {
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::U16(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    for (out, &sample) in frame.iter_mut().zip(&vm.next_frame()) {
                        *out = ((sample * 0.5 + 0.5) * std::u16::MAX as Sample) as u16;
                    }
                }
            }
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::I16(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    for (out, &sample) in frame.iter_mut().zip(&vm.next_frame()) {
                        *out = (sample * std::i16::MAX as Sample) as i16;
                    }
                }
            }
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    for (out, &sample) in frame.iter_mut().zip(&vm.next_frame()) {
                        *out = sample as f32;
                    }
                }
            }
            _ => (),
        }
    });
}
