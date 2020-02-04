use anyhow::Result;
use audio_vm::{Sample, CHANNELS};
use chrono::Local;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use hound::{SampleFormat, WavSpec, WavWriter};
use ringbuf::Consumer;

pub fn main(
    base_filename: &str,
    sample_rate: u32,
    mut consumer: Consumer<Sample>,
    rx: Receiver<bool>,
    _tx: Sender<()>,
) -> Result<()> {
    let spec = WavSpec {
        channels: CHANNELS as _,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer: Option<WavWriter<std::io::BufWriter<std::fs::File>>> = None;
    loop {
        match rx.try_recv() {
            Ok(on) => {
                writer.take().and_then(|w| w.finalize().ok());
                if on {
                    let filename = format!("{}-{}.wav", base_filename, Local::now().to_rfc3339());
                    writer = Some(WavWriter::create(filename, spec)?);
                }
            }
            Err(TryRecvError::Disconnected) => {
                return Ok(());
            }
            Err(TryRecvError::Empty) => {}
        }
        let write = |sample: Sample| {
            let sample = (sample * std::i16::MAX as Sample) as i16;
            writer.as_mut().map(|w| w.write_sample(sample));
            true
        };
        consumer.pop_each(write, None);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
