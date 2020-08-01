use audio_ops::pure::clip;
use audio_program::{compile_program, Context, TextOp};
use audio_vm::{Program, CHANNELS, VM};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rand::prelude::*;
use std::io::Read;

fn main() -> Result<(), anyhow::Error> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;

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

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, &config.into(), &text),
        cpal::SampleFormat::I16 => run::<i16>(&device, &config.into(), &text),
        cpal::SampleFormat::U16 => run::<u16>(&device, &config.into(), &text),
    }
}

fn run<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    text: &str,
) -> Result<(), anyhow::Error>
where
    T: cpal::Sample,
{
    let channels = config.channels as usize;

    let mut vm = VM::new();
    vm.load_program(parse_program(&text, config.sample_rate.0));
    vm.play();

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| write_data(data, channels, &mut vm),
        err_fn,
    )?;
    stream.play()?;

    std::thread::park();

    Ok(())
}

fn write_data<T>(output: &mut [T], channels: usize, vm: &mut VM)
where
    T: cpal::Sample,
{
    for frame in output.chunks_mut(channels) {
        for (sample, &value) in frame.iter_mut().zip(vm.next_frame().iter()) {
            *sample = cpal::Sample::from::<f32>(&(clip(value) as f32));
        }
    }
}

fn parse_program(s: &str, sample_rate: u32) -> Program {
    let ops = s
        .split_whitespace()
        .map(|op| TextOp {
            id: random(),
            op: op.to_string(),
        })
        .collect::<Vec<_>>();
    compile_program(&ops, sample_rate, &mut Context::new())
}
