use anyhow::Result;
use audio_vm::{CHANNELS, Sample};
use chrono::Local;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use hound::{SampleFormat, WavSpec, WavWriter};
use rtrb::Consumer;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const POLL_INTERVAL_MS: u64 = 10;

pub fn main(
    sample_rate: u32,
    mut consumer: Consumer<Sample>,
    recording: Arc<AtomicBool>,
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
                // Stop the audio thread feeding us, keep what it already sent,
                // then close the file.
                recording.store(false, Ordering::Release);
                if let Some(mut w) = writer.take() {
                    while let Ok(sample) = consumer.pop() {
                        w.write_sample((sample * i16::MAX as Sample) as i16).ok();
                    }
                    w.finalize().ok();
                }
                while consumer.pop().is_ok() {}
                if on {
                    let filename = format!("{}.wav", Local::now().to_rfc3339());
                    writer = Some(WavWriter::create(filename, spec)?);
                    recording.store(true, Ordering::Release);
                }
            }
            Err(TryRecvError::Disconnected) => {
                return Ok(());
            }
            Err(TryRecvError::Empty) => {}
        }
        let mut write = |sample: Sample| {
            let sample = (sample * i16::MAX as Sample) as i16;
            writer.as_mut().map(|w| w.write_sample(sample));
            true
        };
        while let Ok(sample) = consumer.pop() {
            write(sample);
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
    }
}
