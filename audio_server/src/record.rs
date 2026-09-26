use anyhow::Result;
use audio_vm::{CHANNELS, Sample};
use chrono::Local;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use hound::{SampleFormat, WavSpec, WavWriter};
use rtrb::Consumer;
use std::{
    fs::File,
    io::{BufWriter, Seek, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const POLL_INTERVAL_MS: u64 = 10;

/// 32-bit float, so recordings keep the engine's resolution instead of being
/// truncated to 16 bits without dither.
fn wav_spec(sample_rate: u32) -> WavSpec {
    WavSpec {
        channels: CHANNELS as _,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    }
}

fn write_available<W: Write + Seek>(writer: &mut WavWriter<W>, consumer: &mut Consumer<Sample>) {
    while let Ok(sample) = consumer.pop() {
        writer.write_sample(sample as f32).ok();
    }
}

pub fn main(
    sample_rate: u32,
    mut consumer: Consumer<Sample>,
    recording: Arc<AtomicBool>,
    rx: Receiver<bool>,
    _tx: Sender<()>,
) -> Result<()> {
    let spec = wav_spec(sample_rate);
    let mut writer: Option<WavWriter<BufWriter<File>>> = None;
    // Stop the audio thread feeding us, keep what it already sent, then close
    // the file.
    let stop = |writer: &mut Option<WavWriter<BufWriter<File>>>,
                consumer: &mut Consumer<Sample>| {
        recording.store(false, Ordering::Release);
        if let Some(mut w) = writer.take() {
            write_available(&mut w, consumer);
            w.finalize().ok();
        }
        while consumer.pop().is_ok() {}
    };
    loop {
        match rx.try_recv() {
            Ok(on) => {
                stop(&mut writer, &mut consumer);
                if on {
                    let filename = format!("{}.wav", Local::now().to_rfc3339());
                    writer = Some(WavWriter::create(filename, spec)?);
                    recording.store(true, Ordering::Release);
                }
            }
            Err(TryRecvError::Disconnected) => {
                stop(&mut writer, &mut consumer);
                return Ok(());
            }
            Err(TryRecvError::Empty) => {}
        }
        if let Some(w) = writer.as_mut() {
            write_available(w, &mut consumer);
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    #[test]
    fn recordings_are_float_and_keep_low_level_detail() {
        let path = std::env::temp_dir().join(format!(
            "sound-garden-record-test-{}.wav",
            std::process::id()
        ));
        let samples: [Sample; 6] = [-1.0, 1.0, 0.25, -0.25, 1e-6, -3.0e-7];
        let (mut producer, mut consumer) = RingBuffer::new(16);
        for &sample in &samples {
            producer.push(sample).unwrap();
        }
        let mut writer = WavWriter::create(&path, wav_spec(48_000)).unwrap();
        write_available(&mut writer, &mut consumer);
        writer.finalize().unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_format, SampleFormat::Float);
        let read = reader
            .samples::<f32>()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        // 1e-6 would be 0 in 16-bit (one step is ~3e-5).
        assert_eq!(read, samples.map(|x| x as f32));
        std::fs::remove_file(path).ok();
    }
}
