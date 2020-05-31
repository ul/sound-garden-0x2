use audio_ops::pure::clip;
use audio_program::{compile_program, Context, TextOp};
use audio_vm::{Program, Sample, CHANNELS, VM};
use hound::{SampleFormat, WavSpec, WavWriter};
use rand::prelude::*;
use std::io::Read;
use std::time::Instant;

fn main() {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .expect("Failed to read stdin");

    let mut args = std::env::args().skip(1);

    let duration = args
        .next()
        .and_then(|x| x.parse::<f64>().ok())
        .expect("Please provide duration in seconds.");
    let output = args.next().expect("Please provide output path.");

    let sample_rate: u32 = 48000;

    let spec = WavSpec {
        channels: CHANNELS as _,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(output, spec).expect("Failed to create a file.");

    let mut vm = VM::new();
    vm.load_program(parse_program(&text, sample_rate));
    vm.play();

    let t = Instant::now();
    for _ in 0..((duration * (sample_rate as f64)) as _) {
        for &sample in &vm.next_frame() {
            let sample = (clip(sample) * std::i16::MAX as Sample) as i16;
            writer
                .write_sample(sample)
                .expect("Failed to write sample.");
        }
    }
    let elapsed = t.elapsed().as_secs_f64();
    println!("Done at x{:.1} speed.", duration / elapsed);
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
