use audio_program::parse_program;
use audio_vm::Sample;
use cpal::traits::{DeviceTrait, EventLoopTrait, HostTrait};
use std::io::Read;

fn main() {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .expect("Failed to read stdin");

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("Failed to get default output device");
    let format = device
        .default_output_format()
        .expect("Failed to get default output format");

    let mut vm = parse_program(&text, format.sample_rate.0);

    let event_loop = host.event_loop();
    let stream_id = event_loop.build_output_stream(&device, &format).unwrap();
    event_loop.play_stream(stream_id.clone()).unwrap();

    event_loop.run(move |id, result| {
        let data = match result {
            Ok(data) => data,
            Err(err) => {
                eprintln!("an error occurred on stream {:?}: {}", id, err);
                return;
            }
        };
        match data {
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::U16(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    for (out, sample) in frame.iter_mut().zip(&vm.next_frame()) {
                        *out = ((sample * 0.5 + 0.5) * std::u16::MAX as Sample) as u16;
                    }
                }
            }
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::I16(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    for (out, sample) in frame.iter_mut().zip(&vm.next_frame()) {
                        *out = (sample * std::i16::MAX as Sample) as i16;
                    }
                }
            }
            cpal::StreamData::Output {
                buffer: cpal::UnknownTypeOutputBuffer::F32(mut buffer),
            } => {
                for frame in buffer.chunks_mut(format.channels as usize) {
                    for (out, sample) in frame.iter_mut().zip(&vm.next_frame()) {
                        *out = *sample as f32;
                    }
                }
            }
            _ => (),
        }
    });
}
