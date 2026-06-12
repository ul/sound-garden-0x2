use audio_ops::pure::clip;
use audio_program::{Context, TextOp, compile_program};
use audio_vm::{CHANNELS, Program, Sample, VM};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::Read;
use std::time::Instant;

fn main() {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .expect("Failed to read stdin");

    let mut stats_enabled = false;
    let args = std::env::args()
        .skip(1)
        .filter(|arg| {
            if arg == "--stats" {
                stats_enabled = true;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();
    let mut args = args.into_iter();

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

    audio_vm::enable_flush_to_zero();

    let mut stats = Stats::new();
    let t = Instant::now();
    for _ in 0..((duration * (sample_rate as f64)) as _) {
        let frame = vm.next_frame();
        if stats_enabled {
            stats.observe(&frame);
        }
        for &sample in &frame {
            let sample = (clip(sample) * i16::MAX as Sample) as i16;
            writer
                .write_sample(sample)
                .expect("Failed to write sample.");
        }
    }
    let elapsed = t.elapsed().as_secs_f64();
    if stats_enabled {
        stats.print();
    }
    println!("Done at x{:.1} speed.", duration / elapsed);
}

struct Stats {
    peak: [Sample; CHANNELS],
    sum_squares: [Sample; CHANNELS],
    sum: [Sample; CHANNELS],
    clipped: [usize; CHANNELS],
    count: usize,
}

impl Stats {
    fn new() -> Self {
        Self {
            peak: [0.0; CHANNELS],
            sum_squares: [0.0; CHANNELS],
            sum: [0.0; CHANNELS],
            clipped: [0; CHANNELS],
            count: 0,
        }
    }

    fn observe(&mut self, frame: &[Sample; CHANNELS]) {
        self.count += 1;
        for (channel, &sample) in frame.iter().enumerate() {
            let abs = sample.abs();
            self.peak[channel] = self.peak[channel].max(abs);
            self.sum_squares[channel] += sample * sample;
            self.sum[channel] += sample;
            if abs > 1.0 {
                self.clipped[channel] += 1;
            }
        }
    }

    fn print(&self) {
        for channel in 0..CHANNELS {
            let rms = (self.sum_squares[channel] / self.count as Sample).sqrt();
            let dc = self.sum[channel] / self.count as Sample;
            let clipped = 100.0 * self.clipped[channel] as Sample / self.count as Sample;
            println!(
                "ch{}: peak {:.3} ({}) rms {:.3} ({}) dc {:.4} clipped {:.2}%",
                channel,
                self.peak[channel],
                dbfs(self.peak[channel]),
                rms,
                dbfs(rms),
                dc,
                clipped,
            );
        }
    }
}

fn dbfs(value: Sample) -> String {
    if value > 0.0 {
        format!("{:.1} dBFS", 20.0 * value.log10())
    } else {
        "-inf".to_string()
    }
}

fn parse_program(s: &str, sample_rate: u32) -> Program {
    let ops = s
        .split_whitespace()
        .map(|op| TextOp {
            id: rand::random(),
            op: op.to_string(),
        })
        .collect::<Vec<_>>();
    compile_program(&ops, sample_rate, &mut Context::new())
}
